//! Slash commands (`/undo`, `/models`, `/<skill>` ...), shared by every interface.
//! They return plain text instead of printing, so the terminal, the full-screen
//! UI and the web UI can each show it their own way.

use anyhow::{Context, Result};
use oblivion_core::{Agent, AgentEvent, ApprovalMode};
use oblivion_mcp::McpHub;
use oblivion_skills::SkillSet;
use oblivion_tools::TodoList;

#[derive(Debug)]
pub enum Command {
    Quit,
    /// Finished; show this text.
    Done(String),
    /// Run this as a request to the agent.
    Run(String),
    /// Run this request with image attachments.
    RunWithImages(String, Vec<oblivion_core::ImageAttachment>),
}

/// Load an image from disk and wrap it in an [`ImageAttachment`].
pub fn load_image_attachment(
    path_str: &str,
    root: &std::path::Path,
) -> Result<oblivion_core::ImageAttachment> {
    use base64::Engine;
    let path = std::path::Path::new(path_str);
    let full_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    if !full_path.exists() {
        anyhow::bail!("image file does not exist: {}", full_path.display());
    }
    let bytes = std::fs::read(&full_path)
        .with_context(|| format!("failed to read image file '{}'", full_path.display()))?;
    let ext = full_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => "image/png",
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let name = full_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image");
    Ok(oblivion_core::ImageAttachment::new(mime, b64).with_name(name))
}

/// The user message for `/skill-name args`: the skill's instructions are
/// included directly, saving the model a round trip to load them.
pub fn skill_request(skills: &SkillSet, name: &str, arg: &str) -> String {
    let loaded = skills.load_text(name).unwrap_or_default();
    let request = if arg.trim().is_empty() {
        "Follow the skill's instructions."
    } else {
        arg.trim()
    };
    format!("The user ran the /{name} skill. Its instructions are loaded below; follow them.\n\n{loaded}\n\nRequest: {request}")
}

pub const HELP: &str = "\
/undo          revert file changes from the last request
/models        show models, cooldowns and today's usage
/todo          show the agent's task list
/compact       summarize older turns now to free up space
/model <text>  only use models whose label contains <text>
/approval <m>  ask | auto-edit | yolo
/clear         start a fresh conversation
/session       show the session id (resume with --resume <id>)
/resume <id>   resume an earlier session
/workspace <p> change working directory for this session
/new [path]    start a fresh session with optional folder
/agents-md     show or update AGENTS.md project instructions
/hardware      show system specs (RAM, GPU, VRAM) and model recommendation
/ollama        manage local models (list, pull <name>, delete <name>, test <name>)
/offline [on|off] toggle or show offline mode (forces local models only)
/routing       show request classification rules and target priorities
/small-model [on|off] toggle or show small-model mode (short prompt, core tools, strict repair)
/spoken [on|off] toggle or show spoken reply style (short answers, code omitted from speech)
/wake-word [on|off] toggle or show wake word detection (hands-free 'Hey Jarvis' keyword spotting)
/image <path> [prompt] attach an image to your request
/screen [prompt] capture your screen with preview and attach it to your request
/stop          emergency stop all computer use desktop operations (halt immediately)
/computer      show computer use allow list and emergency status
/schedule      manage automations (/schedule list, add, pause, resume, delete)
/skills        list skills; run one with /<skill-name> [request]
/mcp           list MCP servers and their tools
/exit          quit";

pub fn models_text(agent: &Agent) -> String {
    agent
        .router
        .status()
        .into_iter()
        .map(|st| {
            let used = match (st.used_today, st.requests_per_day) {
                (Some(u), Some(d)) => format!(" {u}/{d} today"),
                (Some(u), None) => format!(" {u} today"),
                _ => String::new(),
            };
            let checked = match &st.checked {
                Some(c) => format!(" (checked {c})"),
                None => String::new(),
            };
            match st.cooldown {
                Some(d) => format!(
                    "… {} (cooling down {}s){used}{checked}",
                    st.label,
                    d.as_secs()
                ),
                None => format!("✓ {}{used}{checked}", st.label),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn handle_command(
    agent: &mut Agent,
    skills: &SkillSet,
    hub: &McpHub,
    store: Option<&oblivion_store::Store>,
    cmd: &str,
    sink: &mut (dyn FnMut(AgentEvent) + Send),
) -> Result<Command> {
    let (name, arg) = cmd
        .split_once(' ')
        .map(|(a, b)| (a, b.trim()))
        .unwrap_or((cmd, ""));
    let text = match name {
        "exit" | "quit" | "q" => return Ok(Command::Quit),
        name if skills.get(name).is_some() => {
            return Ok(Command::Run(skill_request(skills, name, arg)));
        }
        "skills" => {
            if skills.is_empty() {
                "No skills found.".to_string()
            } else {
                skills
                    .all()
                    .map(|sk| {
                        let d: String = sk.description.chars().take(80).collect();
                        format!("/{:<16} {d}", sk.name)
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        "mcp" => {
            if hub.is_empty() {
                "No MCP servers configured.".to_string()
            } else {
                let mut out = Vec::new();
                for name in hub.names() {
                    match hub.list_tools(&name).await {
                        Ok(t) => out.push(t),
                        Err(e) => out.push(format!("✗ {name}: {e}")),
                    }
                }
                out.join("\n")
            }
        }
        "help" | "h" => HELP.to_string(),
        "stop" => {
            oblivion_tools::trigger_emergency_stop();
            "Emergency stop activated: all desktop computer operations halted.".to_string()
        }
        "computer" => {
            let stopped = oblivion_tools::is_emergency_stopped();
            format!(
                "Computer Use:\n  • Emergency Stop Active: {stopped}\n  • Use `/stop` to halt actions immediately\n  • Use `/reset` or `/resume` to reset emergency stop"
            )
        }
        "reset" | "unstop" => {
            oblivion_tools::reset_emergency_stop();
            "Emergency stop reset: desktop computer operations may resume.".to_string()
        }
        "undo" => match agent.undo_last_turn()? {
            Some(r) => format!(
                "Restored {} file(s), removed {} new file(s).",
                r.restored, r.removed
            ),
            None => "Nothing to undo.".to_string(),
        },
        "models" => models_text(agent),
        "todo" => TodoList::render(&agent.tools.todos().items()),
        "compact" => {
            if agent.compact(true, sink).await? {
                "Earlier turns summarized.".to_string()
            } else {
                "Nothing to compact yet.".to_string()
            }
        }
        "model" => {
            if arg.is_empty() {
                "Usage: /model <text>".to_string()
            } else if agent.router.pin(arg) {
                format!(
                    "Using: {}",
                    agent
                        .router
                        .targets()
                        .iter()
                        .map(|t| t.label())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!("No model matches '{arg}'.")
            }
        }
        "approval" => match ApprovalMode::parse(arg) {
            Some(m) => {
                agent.approval = m;
                format!("Approval mode: {}", approval_name(m))
            }
            None => "Usage: /approval ask | auto-edit | yolo".to_string(),
        },
        "clear" => {
            agent.clear();
            "Conversation cleared.".to_string()
        }
        "session" => format!("Session {}", agent.session.id),
        "workspace" | "cd" => {
            if arg.is_empty() {
                format!("Current workspace: {}", agent.tools.root().display())
            } else {
                let p = std::path::PathBuf::from(arg);
                if !p.is_dir() {
                    format!("Directory not found: {}", p.display())
                } else {
                    agent.set_workspace(p.clone());
                    format!("Workspace updated to: {}", p.display())
                }
            }
        }
        "new" => {
            let ws = if arg.is_empty() {
                agent.tools.root().to_path_buf()
            } else {
                std::path::PathBuf::from(arg)
            };
            if !ws.is_dir() {
                format!("Directory not found: {}", ws.display())
            } else {
                agent.new_session(ws.clone());
                format!(
                    "Started new session {} in {}",
                    agent.session.id,
                    ws.display()
                )
            }
        }
        "resume" => {
            if arg.is_empty() {
                if oblivion_tools::is_emergency_stopped() {
                    oblivion_tools::reset_emergency_stop();
                    "Emergency stop reset: desktop computer operations may resume.".to_string()
                } else {
                    "Usage: /resume <session_id>".to_string()
                }
            } else if let Some(st) = store {
                match st.load_session(arg) {
                    Ok(loaded) => {
                        let count = loaded.messages.len();
                        let ws = loaded.workspace.display().to_string();
                        agent.resume(loaded);
                        format!("Resumed session {arg} ({count} messages, workspace: {ws})")
                    }
                    Err(e) => format!("Failed to load session {arg}: {e}"),
                }
            } else {
                "Cannot resume: session store is not available.".to_string()
            }
        }
        "agents-md" => {
            let root = agent.tools.root();
            let current = oblivion_core::prompt::project_instructions(root);
            if arg == "update" || arg == "apply" {
                let recent_memories = if let Some(st) = store {
                    st.list_memories(None, 50).unwrap_or_default()
                } else {
                    Vec::new()
                };
                let proposed: Vec<oblivion_core::extract::ProposedMemory> = recent_memories
                    .into_iter()
                    .map(|m| {
                        oblivion_core::extract::ProposedMemory::new(
                            m.mem_type,
                            m.text,
                            m.importance,
                        )
                    })
                    .collect();
                if let Some(prop) = agent.propose_agents_md_update(&proposed) {
                    let count = prop.additions.len();
                    let filename = prop.path.file_name().unwrap_or_default().to_string_lossy();
                    match agent.apply_agents_md_update(&prop) {
                        Ok(()) => {
                            let items = prop
                                .additions
                                .iter()
                                .map(|a| format!("  + {a}"))
                                .collect::<Vec<_>>()
                                .join("\n");
                            format!(
                                "✓ Updated {filename} with {count} learned instructions:\n{items}"
                            )
                        }
                        Err(e) => format!("✗ Failed to update {filename}: {e}"),
                    }
                } else {
                    "AGENTS.md is already up to date with all learned project facts.".to_string()
                }
            } else if arg == "show" {
                match current {
                    Some((file, content)) => format!("Instructions from {file}:\n\n{content}"),
                    None => format!(
                        "No instructions file (AGENTS.md or OBLIVION.md) found in {}.",
                        root.display()
                    ),
                }
            } else {
                let (file_status, count_text) = match current {
                    Some((file, content)) => (
                        format!("Loaded from {file}"),
                        format!("{} chars, {} lines", content.len(), content.lines().count()),
                    ),
                    None => (
                        "No instructions file active".to_string(),
                        "0 chars".to_string(),
                    ),
                };
                format!(
                    "Project instructions: {file_status} ({count_text})\nWorkspace: {}\n\nCommands:\n/agents-md show   - view current instruction file\n/agents-md update - check learned memories and apply updates to AGENTS.md",
                    root.display()
                )
            }
        }
        "hardware" | "specs" => {
            let hw = oblivion_core::detect_hardware();
            let rec = oblivion_core::recommend_model(&hw);
            let vram_str = match (hw.vram_gb, hw.is_integrated_gpu) {
                (Some(v), true) => format!(" ({v:.1} GB, shared/integrated)"),
                (Some(v), false) => format!(" ({v:.1} GB dedicated VRAM)"),
                (None, _) => String::new(),
            };
            format!(
                "Hardware Specs:\n  • RAM:  {:.1} GB total ({:.1} GB available)\n  • GPU:  {}{vram_str}\n  • Disk: {:.1} GB free\n\nLocal Model Recommendation:\n  • Recommended size:  {}\n  • Suggested models:  {}\n  • Quantization:      {} (~{:.1} GB needed)\n  • Fits in VRAM:      {}\n  • Trade-off:         {}",
                hw.total_ram_gb,
                hw.available_ram_gb,
                hw.gpu_name,
                hw.free_disk_gb,
                rec.recommended_size,
                rec.recommended_models.join(", "),
                rec.recommended_quantization,
                rec.estimated_ram_required_gb,
                if rec.fits_in_vram { "Yes (fully offloaded to GPU)" } else { "No (runs in system RAM)" },
                rec.tradeoff_explanation
            )
        }
        "ollama" | "local-models" => {
            let client = oblivion_core::OllamaClient::new("http://localhost:11434");
            let status = client.check_status().await;
            if !status.running {
                let err = status.error.unwrap_or_else(|| "cannot connect".into());
                format!(
                    "Ollama is not running at {} ({err}).\nInstall or start Ollama (`ollama serve`) to run local models.",
                    client.endpoint()
                )
            } else {
                let parts: Vec<&str> = arg.split_whitespace().collect();
                match parts.first().copied().unwrap_or("") {
                    "" | "list" => match client.list_models().await {
                        Ok(models) if models.is_empty() => {
                            "Ollama is running, but no local models are installed yet. Try `/ollama pull <name>` (e.g. `/ollama pull qwen2.5-coder:7b`).".to_string()
                        }
                        Ok(models) => {
                            let mut lines = vec![format!(
                                "Local models in Ollama (version {}):",
                                status.version.unwrap_or_else(|| "unknown".into())
                            )];
                            for m in models {
                                let param = m
                                    .details
                                    .as_ref()
                                    .and_then(|d| d.parameter_size.clone())
                                    .unwrap_or_else(|| "?".into());
                                let quant = m
                                    .details
                                    .as_ref()
                                    .and_then(|d| d.quantization_level.clone())
                                    .unwrap_or_default();
                                lines.push(format!(
                                    "  • {:<24} {:>8}  {:<6} {:<8}",
                                    m.name, m.size_formatted, param, quant
                                ));
                            }
                            lines.join("\n")
                        }
                        Err(e) => format!("Failed to list Ollama models: {e}"),
                    },
                    "pull" => {
                        let name = parts.get(1).copied().unwrap_or("");
                        if name.is_empty() {
                            "Usage: /ollama pull <model-name> (e.g. /ollama pull qwen2.5-coder:7b)"
                                .to_string()
                        } else {
                            match client.pull_model_stream(name).await {
                                Ok(mut stream) => {
                                    use futures_util::StreamExt;
                                    let mut last_status = String::new();
                                    while let Some(item) = stream.next().await {
                                        match item {
                                            Ok(p) => {
                                                if let Some(pct) = p.percent {
                                                    last_status = format!("{}: {:.1}%", p.status, pct);
                                                } else {
                                                    last_status = p.status;
                                                }
                                            }
                                            Err(e) => {
                                                return Ok(Command::Done(format!("Pull failed: {e}")));
                                            }
                                        }
                                    }
                                    format!("Successfully pulled {name}. Status: {last_status}")
                                }
                                Err(e) => format!("Failed to start pull for {name}: {e}"),
                            }
                        }
                    }
                    "delete" | "rm" => {
                        let name = parts.get(1).copied().unwrap_or("");
                        if name.is_empty() {
                            "Usage: /ollama delete <model-name>".to_string()
                        } else {
                            match client.delete_model(name).await {
                                Ok(()) => format!("Deleted model {name} from Ollama."),
                                Err(e) => format!("Failed to delete {name}: {e}"),
                            }
                        }
                    }
                    "test" | "benchmark" => {
                        let name = parts.get(1).copied().unwrap_or("");
                        if name.is_empty() {
                            "Usage: /ollama test <model-name>".to_string()
                        } else {
                            match client.speed_test(name, None).await {
                                Ok(res) => {
                                    format!(
                                        "Speed test for {name}:\n  • Generation speed:    {:.1} tokens/sec ({} tokens in {:.0}ms)\n  • Prompt evaluation:   {:.1} tokens/sec ({} tokens in {:.0}ms)\n  • Model load duration: {:.0}ms\n  • Total roundtrip:     {:.0}ms\n  • Output sample:       {}",
                                        res.tokens_per_second,
                                        res.eval_count,
                                        res.eval_duration_ms,
                                        res.prompt_eval_tokens_per_second,
                                        res.prompt_eval_count,
                                        res.prompt_eval_duration_ms,
                                        res.load_duration_ms,
                                        res.total_duration_ms,
                                        res.sample_text
                                    )
                                }
                                Err(e) => format!("Failed to run speed test on {name}: {e}"),
                            }
                        }
                    }
                    other => format!(
                        "Unknown /ollama subcommand '{other}'. Try /ollama list, /ollama pull <name>, /ollama delete <name>, /ollama test <name>"
                    ),
                }
            }
        }
        "offline" => match arg {
            "on" | "true" | "enable" | "1" => {
                agent.offline = true;
                "Offline mode enabled. Only local models (Ollama/localhost) will be used."
                    .to_string()
            }
            "off" | "false" | "disable" | "0" => {
                agent.offline = false;
                "Offline mode disabled. Configured cloud and local models will be used.".to_string()
            }
            "" => {
                format!(
                    "Offline mode is currently {}.\nUse `/offline on` or `/offline off` to change.",
                    if agent.offline {
                        "ENABLED (local only)"
                    } else {
                        "DISABLED (cloud + local)"
                    }
                )
            }
            other => format!("Unknown argument '{other}'. Use `/offline on` or `/offline off`."),
        },
        "routing" => {
            let rules = &agent.routing_rules;
            let status = if agent.offline {
                "ENABLED (Forces all requests to local models only)"
            } else {
                "DISABLED (Cloud and local models based on request class)"
            };
            format!(
                "Routing & Privacy Rules:\n  • Offline Mode: {}\n\nRequest Classes & Target Priorities:\n  • Private (sensitive/credentials):  {}\n  • Vision (image inputs):           {}\n  • Long Context (>16k tokens):       {}\n  • Hard Reasoning (math, debug):    {}\n  • Simple (greetings, syntax):       {}",
                status,
                rules.private.join(", "),
                rules.vision.join(", "),
                rules.long_context.join(", "),
                rules.hard.join(", "),
                rules.simple.join(", ")
            )
        }
        "small-model" | "small" => match arg {
            "on" | "true" | "enable" | "1" => {
                agent.small_model = true;
                "Small-model mode enabled (compact system prompt, core tools only, robust argument repair).".to_string()
            }
            "off" | "false" | "disable" | "0" => {
                agent.small_model = false;
                "Small-model mode disabled (full prompt and all tools active).".to_string()
            }
            "" => {
                let status = if agent.is_small_model_active() {
                    if agent.small_model {
                        "ENABLED (manually turned on)"
                    } else {
                        "ENABLED (automatically detected based on model name)"
                    }
                } else {
                    "DISABLED"
                };
                format!(
                    "Small-model mode is currently {status}.\nUse `/small-model on` or `/small-model off` to change."
                )
            }
            other => {
                format!("Unknown argument '{other}'. Use `/small-model on` or `/small-model off`.")
            }
        },
        "spoken" | "voice" => match arg {
            "on" | "true" | "enable" | "1" => {
                agent.spoken_reply = true;
                "Spoken reply style enabled (short conversational answers, code omitted from speech).".to_string()
            }
            "off" | "false" | "disable" | "0" => {
                agent.spoken_reply = false;
                "Spoken reply style disabled.".to_string()
            }
            "" => {
                let status = if agent.spoken_reply {
                    "ENABLED"
                } else {
                    "DISABLED"
                };
                format!(
                    "Spoken reply style is currently {status}.\nUse `/spoken on` or `/spoken off` to change."
                )
            }
            other => {
                format!("Unknown argument '{other}'. Use `/spoken on` or `/spoken off`.")
            }
        },
        "wake-word" | "wakeword" => match arg {
            "on" | "true" | "enable" | "1" => {
                agent.wake_word_enabled = true;
                "Wake word detection enabled (hands-free 'Hey Jarvis' keyword spotting)."
                    .to_string()
            }
            "off" | "false" | "disable" | "0" => {
                agent.wake_word_enabled = false;
                "Wake word detection disabled (push-to-talk only).".to_string()
            }
            "" => {
                let status = if agent.wake_word_enabled {
                    "ENABLED"
                } else {
                    "DISABLED"
                };
                format!(
                    "Wake word detection is currently {status}.\nUse `/wake-word on` or `/wake-word off` to change."
                )
            }
            other => {
                format!("Unknown argument '{other}'. Use `/wake-word on` or `/wake-word off`.")
            }
        },
        "image" => {
            if arg.is_empty() {
                return Ok(Command::Done("Usage: /image <path> [prompt]".to_string()));
            }
            let (path_str, prompt_text) = if let Some(rest) = arg.strip_prefix('"') {
                if let Some(end) = rest.find('"') {
                    let p = &rest[..end];
                    let rem = rest[end + 1..].trim();
                    (p, rem)
                } else {
                    (arg, "")
                }
            } else {
                arg.split_once(' ').unwrap_or((arg, ""))
            };
            let attachment = load_image_attachment(path_str, agent.tools.root())?;
            let prompt = if prompt_text.is_empty() {
                "Describe this image in detail.".to_string()
            } else {
                prompt_text.to_string()
            };
            return Ok(Command::RunWithImages(prompt, vec![attachment]));
        }
        "screen" => {
            let attachment = crate::screen::capture_screen()?;
            let prompt = if arg.is_empty() {
                "Look at my screen and describe what you see.".to_string()
            } else {
                arg.to_string()
            };
            return Ok(Command::RunWithImages(prompt, vec![attachment]));
        }
        "schedule" | "schedules" => {
            let Some(st) = store else {
                return Ok(Command::Done(
                    "Storage is not available for scheduled tasks.".into(),
                ));
            };
            let (subcmd, rest) = arg
                .split_once(' ')
                .map(|(a, b)| (a, b.trim()))
                .unwrap_or((arg, ""));
            match subcmd {
                "list" | "" => {
                    let tasks = st.list_scheduled_tasks(false)?;
                    if tasks.is_empty() {
                        "No scheduled tasks configured. Add one with: /schedule add \"<schedule>\" <prompt>"
                            .to_string()
                    } else {
                        let mut lines = vec![format!("Scheduled Tasks ({}):", tasks.len())];
                        for t in tasks {
                            let status = if t.enabled { "active" } else { "paused" };
                            let next = t
                                .next_run
                                .map(|n| n.format("%Y-%m-%d %H:%M UTC").to_string())
                                .unwrap_or_else(|| "none".to_string());
                            let last = t.last_outcome.as_deref().unwrap_or("never run");
                            lines.push(format!(
                                "  • [{}] {} — \"{}\" (Status: {status}, Next: {next}, Last: {last})",
                                t.id, t.name, t.schedule
                            ));
                        }
                        lines.join("\n")
                    }
                }
                "pause" => {
                    if rest.is_empty() {
                        "Usage: /schedule pause <task_id>".to_string()
                    } else {
                        let update = oblivion_store::UpdateScheduledTask {
                            enabled: Some(false),
                            ..Default::default()
                        };
                        match st.update_scheduled_task(rest, &update) {
                            Ok(t) => format!("Paused task '{}' [{}]", t.name, t.id),
                            Err(e) => format!("Failed to pause task: {e}"),
                        }
                    }
                }
                "resume" => {
                    if rest.is_empty() {
                        "Usage: /schedule resume <task_id>".to_string()
                    } else {
                        let update = oblivion_store::UpdateScheduledTask {
                            enabled: Some(true),
                            ..Default::default()
                        };
                        match st.update_scheduled_task(rest, &update) {
                            Ok(t) => format!("Resumed task '{}' [{}]", t.name, t.id),
                            Err(e) => format!("Failed to resume task: {e}"),
                        }
                    }
                }
                "delete" | "rm" => {
                    if rest.is_empty() {
                        "Usage: /schedule delete <task_id>".to_string()
                    } else {
                        match st.delete_scheduled_task(rest) {
                            Ok(true) => format!("Deleted scheduled task '{rest}'."),
                            Ok(false) => format!("Task not found: '{rest}'"),
                            Err(e) => format!("Failed to delete task: {e}"),
                        }
                    }
                }
                "add" => {
                    let parts = split_quoted_args(rest);
                    if parts.len() < 2 {
                        "Usage: /schedule add \"<schedule>\" <prompt>  OR  /schedule add \"<name>\" \"<schedule>\" <prompt>"
                            .to_string()
                    } else {
                        let (name, sched_expr, prompt) = if parts.len() >= 3 {
                            (parts[0].clone(), parts[1].clone(), parts[2..].join(" "))
                        } else {
                            (
                                format!("Task ({})", parts[0]),
                                parts[0].clone(),
                                parts[1..].join(" "),
                            )
                        };
                        match oblivion_core::schedule::next_run_after(
                            &sched_expr,
                            chrono::Utc::now(),
                        ) {
                            Ok(next_run) => {
                                let mut new_task = oblivion_store::NewScheduledTask::new(
                                    &name,
                                    &prompt,
                                    &sched_expr,
                                )
                                .with_workspace(agent.tools.root().to_string_lossy());
                                if let Some(nr) = next_run {
                                    new_task.next_run = Some(nr);
                                }
                                match st.add_scheduled_task(&new_task) {
                                    Ok(task) => format!(
                                        "Created scheduled task '{}' [{}] with schedule '{}'. Next run: {}",
                                        task.name,
                                        task.id,
                                        task.schedule,
                                        task.next_run
                                            .map(|n| n.to_rfc3339())
                                            .unwrap_or_else(|| "none".to_string())
                                    ),
                                    Err(e) => format!("Failed to create task: {e}"),
                                }
                            }
                            Err(e) => format!("Invalid schedule expression '{sched_expr}': {e}"),
                        }
                    }
                }
                other => format!(
                    "Unknown schedule subcommand '{other}'. Usage: /schedule list | add | pause | resume | delete"
                ),
            }
        }
        other => format!("Unknown command /{other}. Try /help or /skills."),
    };
    Ok(Command::Done(text))
}

pub fn approval_name(m: ApprovalMode) -> &'static str {
    match m {
        ApprovalMode::Ask => "ask",
        ApprovalMode::AutoEdit => "auto-edit",
        ApprovalMode::Yolo => "yolo",
    }
}

pub fn split_quoted_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            chars.next();
            let mut s = String::new();
            while let Some(&ch) = chars.peek() {
                if ch == quote {
                    chars.next();
                    break;
                }
                s.push(ch);
                chars.next();
            }
            args.push(s);
        } else {
            let mut s = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                s.push(ch);
                chars.next();
            }
            args.push(s);
        }
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_image_attachment_png() {
        let tmp = tempfile::tempdir().unwrap();
        let img_path = tmp.path().join("test_shot.png");
        std::fs::write(&img_path, b"\x89PNG\r\n\x1a\nfakeimagebytes").unwrap();

        let att = load_image_attachment("test_shot.png", tmp.path()).unwrap();
        assert_eq!(att.mime_type, "image/png");
        assert_eq!(att.name.as_deref(), Some("test_shot.png"));
        assert!(!att.data_base64.is_empty());
    }

    #[test]
    fn test_load_image_attachment_jpg() {
        let tmp = tempfile::tempdir().unwrap();
        let img_path = tmp.path().join("photo.jpeg");
        std::fs::write(&img_path, b"fakejpeg").unwrap();

        let att = load_image_attachment(img_path.to_str().unwrap(), tmp.path()).unwrap();
        assert_eq!(att.mime_type, "image/jpeg");
        assert_eq!(att.name.as_deref(), Some("photo.jpeg"));
    }

    #[test]
    fn test_load_image_attachment_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let res = load_image_attachment("nonexistent.png", tmp.path());
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_screen_command_runs_with_images() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, _rx) = crate::app::channels();
        let approvals = crate::app::Approvals::new(tx);
        let tools = oblivion_tools::ToolBox::new(
            tmp.path(),
            oblivion_tools::Shell::Cmd,
            tmp.path().join(".snap"),
        );
        let mut a = Agent::new(
            oblivion_providers::Router::new(vec![]),
            tools,
            approvals.approver(),
            ApprovalMode::Ask,
            10,
            "system".into(),
        );
        let skills = SkillSet::default();
        let hub = McpHub::new(vec![]);
        let mut sink = |_e: AgentEvent| {};

        std::env::set_var("OBLIVION_MOCK_SCREEN_CAPTURE", "1");
        let res = handle_command(
            &mut a,
            &skills,
            &hub,
            None,
            "screen what is open",
            &mut sink,
        )
        .await
        .unwrap();
        std::env::remove_var("OBLIVION_MOCK_SCREEN_CAPTURE");

        match res {
            Command::RunWithImages(prompt, images) => {
                assert_eq!(prompt, "what is open");
                assert_eq!(images.len(), 1);
                assert_eq!(images[0].mime_type, "image/png");
            }
            other => panic!("expected Command::RunWithImages, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_schedule_command_crud() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, _rx) = crate::app::channels();
        let approvals = crate::app::Approvals::new(tx);
        let tools = oblivion_tools::ToolBox::new(
            tmp.path(),
            oblivion_tools::Shell::Cmd,
            tmp.path().join(".snap"),
        );
        let mut a = Agent::new(
            oblivion_providers::Router::new(vec![]),
            tools,
            approvals.approver(),
            ApprovalMode::Ask,
            10,
            "system".into(),
        );
        let skills = SkillSet::default();
        let hub = McpHub::new(vec![]);
        let mut sink = |_e: AgentEvent| {};
        let store = oblivion_store::Store::in_memory().unwrap();

        // 1. Initial list empty
        let res = handle_command(
            &mut a,
            &skills,
            &hub,
            Some(&store),
            "schedule list",
            &mut sink,
        )
        .await
        .unwrap();
        if let Command::Done(txt) = res {
            assert!(txt.contains("No scheduled tasks"));
        } else {
            panic!("expected Command::Done");
        }

        // 2. Add task
        let res = handle_command(
            &mut a,
            &skills,
            &hub,
            Some(&store),
            "schedule add \"Daily Check\" \"every 1d\" Run healthcheck",
            &mut sink,
        )
        .await
        .unwrap();
        let task_id = if let Command::Done(txt) = res {
            assert!(txt.contains("Created scheduled task 'Daily Check'"));
            let start = txt.find('[').unwrap() + 1;
            let end = txt.find(']').unwrap();
            txt[start..end].to_string()
        } else {
            panic!("expected Command::Done");
        };

        // 3. List shows the task
        let res = handle_command(
            &mut a,
            &skills,
            &hub,
            Some(&store),
            "schedule list",
            &mut sink,
        )
        .await
        .unwrap();
        if let Command::Done(txt) = res {
            assert!(txt.contains("Daily Check"));
            assert!(txt.contains("active"));
        } else {
            panic!("expected Command::Done");
        }

        // 4. Pause task
        let res = handle_command(
            &mut a,
            &skills,
            &hub,
            Some(&store),
            &format!("schedule pause {task_id}"),
            &mut sink,
        )
        .await
        .unwrap();
        if let Command::Done(txt) = res {
            assert!(txt.contains("Paused task"));
        } else {
            panic!("expected Command::Done");
        }

        // 5. Resume task
        let res = handle_command(
            &mut a,
            &skills,
            &hub,
            Some(&store),
            &format!("schedule resume {task_id}"),
            &mut sink,
        )
        .await
        .unwrap();
        if let Command::Done(txt) = res {
            assert!(txt.contains("Resumed task"));
        } else {
            panic!("expected Command::Done");
        }

        // 6. Delete task
        let res = handle_command(
            &mut a,
            &skills,
            &hub,
            Some(&store),
            &format!("schedule delete {task_id}"),
            &mut sink,
        )
        .await
        .unwrap();
        if let Command::Done(txt) = res {
            assert!(txt.contains("Deleted scheduled task"));
        } else {
            panic!("expected Command::Done");
        }
    }
}
