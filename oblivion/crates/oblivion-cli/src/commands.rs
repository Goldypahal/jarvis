//! Slash commands (`/undo`, `/models`, `/<skill>` ...), shared by every interface.
//! They return plain text instead of printing, so the terminal, the full-screen
//! UI and the web UI can each show it their own way.

use anyhow::Result;
use oblivion_core::{Agent, AgentEvent, ApprovalMode};
use oblivion_mcp::McpHub;
use oblivion_skills::SkillSet;
use oblivion_tools::TodoList;

pub enum Command {
    Quit,
    /// Finished; show this text.
    Done(String),
    /// Run this as a request to the agent.
    Run(String),
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
                "Usage: /resume <session_id>".to_string()
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
