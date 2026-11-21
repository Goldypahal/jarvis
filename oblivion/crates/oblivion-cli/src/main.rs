//! `oblivion` command-line interface (SRS FR-70/71).

mod app;
pub mod autostart;
mod commands;
mod keys;
pub mod screen;
#[cfg(test)]
mod testkit;
pub mod trace;
mod tui;
mod web;
pub mod wizard;

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use clap::Parser;
use commands::{handle_command, skill_request, Command};
use oblivion_core::config::{self, Config, DEFAULT_CONFIG};
use oblivion_core::{prompt, Agent, AgentEvent, ApprovalMode, Approver, Decision, Rules};
use oblivion_mcp::{BrowserMode, BrowserSettings, BrowserTool, McpHub};
use oblivion_providers::{Router, Target, UsageLedger};
use oblivion_skills::{GithubSource, SkillSet, SkillSource};
use oblivion_store::Store;
use oblivion_tools::{normalize_path, SearchBackend, Shell, ToolBox};
use tokio::sync::Notify;

#[derive(Parser, Debug)]
#[command(
    name = "oblivion",
    version,
    about = "A personal AI agent that runs on free model APIs"
)]
struct Cli {
    /// Run this task once and exit. Without it, starts an interactive chat.
    prompt: Vec<String>,

    /// Only use models whose "provider/model" label contains this text (e.g. "groq").
    #[arg(short, long)]
    model: Option<String>,

    /// Approval mode: ask, auto-edit or yolo.
    #[arg(short, long)]
    approval: Option<String>,

    /// Shortcut for --approval yolo. Never asks. Use only in a throwaway folder.
    #[arg(long)]
    yes: bool,

    /// Workspace folder (default: current folder).
    #[arg(short = 'C', long)]
    cwd: Option<PathBuf>,

    /// Config file (default: ~/.oblivion/oblivion.toml).
    #[arg(long)]
    config: Option<PathBuf>,

    /// Resume a saved session by id (see --sessions).
    #[arg(long)]
    resume: Option<String>,

    /// Write a starter config file and exit.
    #[arg(long)]
    init: bool,

    /// List configured models and which providers are missing keys, then exit.
    #[arg(long)]
    models: bool,

    /// List saved sessions, then exit.
    #[arg(long)]
    sessions: bool,

    /// Save a provider's API key in the system key store (Windows Credential Manager), then exit.
    #[arg(long, value_name = "PROVIDER")]
    set_key: Option<String>,

    /// Delete a provider's saved API key, then exit.
    #[arg(long, value_name = "PROVIDER")]
    delete_key: Option<String>,

    /// Detect system hardware (RAM, GPU, VRAM) and print model recommendation, then exit.
    #[arg(long, short = 'H')]
    hardware: bool,

    /// First-run onboarding wizard (pick workspace, set keys or offline mode, autostart).
    #[arg(long)]
    wizard: bool,

    /// Manage Windows autostart on boot (on, off, or status).
    #[arg(long, value_name = "ACTION")]
    autostart: Option<String>,

    /// Force offline mode: send zero data to cloud models, use only local models (Ollama).
    #[arg(long)]
    offline: bool,

    /// Show today's requests and tokens per model, then exit.
    #[arg(long)]
    usage: bool,

    /// List available skills (and any problems loading them), then exit.
    #[arg(long)]
    skills: bool,

    /// Install skills from a GitHub folder, e.g. https://github.com/anthropics/skills/tree/main/skills/mcp-builder
    #[arg(long, value_name = "GITHUB_URL")]
    install_skill: Option<String>,

    /// Connect to each configured MCP server and list its tools, then exit.
    #[arg(long)]
    mcp: bool,

    /// Turn on the web browser tool and test it, then exit. MODE is "own" (a separate
    /// browser window that keeps its own logins; default) or "extension" (your Chrome tabs).
    #[arg(long, value_name = "MODE", num_args = 0..=1, default_missing_value = "own")]
    setup_browser: Option<String>,

    /// Open Oblivion in your web browser (a local page on 127.0.0.1).
    #[arg(long, conflicts_with = "tui")]
    web: bool,

    /// Port for --web (another free port is used if it is taken).
    #[arg(long, default_value_t = 7878)]
    port: u16,

    /// With --web: print the link instead of opening the browser.
    #[arg(long)]
    no_open: bool,

    /// Full-screen terminal interface (chat, task list and helpers side by side).
    #[arg(long)]
    tui: bool,

    /// Small-model mode (shorter prompt, core tools only, strict output repair).
    #[arg(long)]
    small_model: bool,

    /// Attach an image file to the request.
    #[arg(long, value_name = "IMAGE_PATH")]
    image: Option<PathBuf>,

    /// Capture a screenshot of the primary display and attach it to the request.
    #[arg(long)]
    screen: bool,

    /// Print a run's trace log (JSON lines) by run id, then exit. Use "list" to show traces.
    #[arg(long, value_name = "ID")]
    trace: Option<String>,
}

mod style {
    pub const DIM: &str = "\x1b[2m";
    pub const BOLD: &str = "\x1b[1m";
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const CYAN: &str = "\x1b[36m";
    pub const RESET: &str = "\x1b[0m";
}
use style::*;

struct TerminalApprover;

impl Approver for TerminalApprover {
    fn approve(&self, tool: &str, summary: &str) -> Decision {
        let stdin = std::io::stdin();
        if !stdin.is_terminal() {
            eprintln!("{YELLOW}! {tool} needs approval but there is no terminal; declined. Use --approval auto-edit or --yes.{RESET}");
            return Decision::No;
        }
        loop {
            print!("\n{YELLOW}? Allow {BOLD}{summary}{RESET}{YELLOW}  [y]es / [n]o / [a]lways for {tool}: {RESET}");
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
                return Decision::No;
            }
            match line.trim().to_ascii_lowercase().as_str() {
                "y" | "yes" => return Decision::Yes,
                "n" | "no" => return Decision::No,
                "a" | "always" => return Decision::Always,
                _ => println!("Please type y, n or a."),
            }
        }
    }
}

struct TracingApprover {
    inner: Box<dyn Approver>,
    tracer: Arc<Mutex<Option<Arc<trace::Tracer>>>>,
}

impl Approver for TracingApprover {
    fn approve(&self, tool: &str, summary: &str) -> Decision {
        let decision = self.inner.approve(tool, summary);
        if let Some(tracer) = self.tracer.lock().unwrap().as_ref() {
            let dec_str = match decision {
                Decision::Yes => "yes",
                Decision::No => "no",
                Decision::Always => "always",
            };
            tracer.record_approval(0, tool, summary, dec_str);
        }
        decision
    }
}

fn print_event(e: AgentEvent) {
    let mut out = std::io::stdout();
    match e {
        AgentEvent::Text(t) => {
            let _ = write!(out, "{t}");
        }
        AgentEvent::TextEnd => {
            let _ = writeln!(out);
        }
        AgentEvent::ToolStart { name, summary, .. } => {
            let _ = writeln!(out, "{CYAN}● {name}{RESET} {DIM}{summary}{RESET}");
        }
        AgentEvent::ToolEnd {
            preview, is_error, ..
        } => {
            let color = if is_error { RED } else { DIM };
            for line in preview.lines() {
                let _ = writeln!(out, "  {color}│ {line}{RESET}");
            }
        }
        AgentEvent::Blocked { name, rule } => {
            let _ = writeln!(out, "{RED}  ⛔ {name} blocked by rule {rule}{RESET}");
        }
        AgentEvent::Declined { name } => {
            let _ = writeln!(out, "{YELLOW}  ✗ declined {name}{RESET}");
        }
        AgentEvent::Fallback { from, reason } => {
            // "rate limited (retry in 15s): {long provider JSON}" -> "rate limited (retry in 15s)"
            let short: String = reason
                .split(": {")
                .next()
                .unwrap_or(&reason)
                .chars()
                .take(120)
                .collect();
            let _ = writeln!(
                out,
                "{YELLOW}  ↷ {from} failed ({short}); trying next model{RESET}"
            );
        }
        AgentEvent::Model(m) => {
            let _ = writeln!(out, "{DIM}  [{m}]{RESET}");
        }
        AgentEvent::Waiting(secs) => {
            let _ = writeln!(
                out,
                "{YELLOW}  ⏳ all models are at their per-minute limit; waiting {secs}s{RESET}"
            );
        }
        AgentEvent::Sub {
            index,
            label,
            event,
        } => {
            let tag = format!("{CYAN}  ↳ [{index}] {label}{RESET}");
            match *event {
                AgentEvent::ToolStart { summary, .. } => {
                    let _ = writeln!(out, "{tag} {DIM}{summary}{RESET}");
                }
                AgentEvent::ToolEnd {
                    is_error: true,
                    preview,
                    ..
                } => {
                    let first = preview.lines().next().unwrap_or("").to_string();
                    let _ = writeln!(out, "{tag} {RED}{first}{RESET}");
                }
                AgentEvent::Model(m) => {
                    let _ = writeln!(out, "{tag} {DIM}[{m}]{RESET}");
                }
                AgentEvent::Waiting(s) => {
                    let _ = writeln!(out, "{tag} {YELLOW}⏳ waiting {s}s for rate limit{RESET}");
                }
                AgentEvent::Fallback { from, .. } => {
                    let _ = writeln!(
                        out,
                        "{tag} {YELLOW}↷ {from} unavailable, trying next{RESET}"
                    );
                }
                AgentEvent::Notice(n) => {
                    let _ = writeln!(out, "{tag} {DIM}{n}{RESET}");
                }
                _ => {}
            }
        }
        AgentEvent::Notice(n) => {
            let _ = writeln!(out, "{YELLOW}{n}{RESET}");
        }
    }
    let _ = out.flush();
}

fn init_config(path: &PathBuf) -> Result<()> {
    if path.exists() {
        bail!(
            "{} already exists; edit it or delete it first",
            path.display()
        );
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, DEFAULT_CONFIG)?;
    println!("Wrote {}", path.display());
    println!("Next: set at least one API key, e.g. on Windows PowerShell:");
    println!("  setx OPENROUTER_API_KEY \"sk-or-...\"   (then open a new terminal)");
    println!("Then run `oblivion --models` to check, and `oblivion` to start.");
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{RED}error:{RESET} {e:#}");
        std::process::exit(1);
    }
}

/// The classic Windows console shows colour codes as `←[1m` unless the
/// program switches on "virtual terminal processing" for its window.
#[cfg(windows)]
fn enable_ansi_colors() {
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };
    for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: plain Win32 calls on this process's own standard handles;
        // `mode` is a valid local the API writes into.
        unsafe {
            let handle = GetStdHandle(which);
            let mut mode: u32 = 0;
            if !handle.is_null() && GetConsoleMode(handle, &mut mode) != 0 {
                SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
        }
    }
}

#[cfg(not(windows))]
fn enable_ansi_colors() {}

fn print_hardware() {
    let hw = oblivion_core::detect_hardware();
    let rec = oblivion_core::recommend_model(&hw);

    println!("{BOLD}Hardware Specs{RESET}");
    println!(
        "  • RAM:  {:.1} GB total ({:.1} GB available)",
        hw.total_ram_gb, hw.available_ram_gb
    );
    let vram_str = match (hw.vram_gb, hw.is_integrated_gpu) {
        (Some(v), true) => format!(" ({v:.1} GB, shared/integrated)"),
        (Some(v), false) => format!(" ({v:.1} GB dedicated VRAM)"),
        (None, _) => String::new(),
    };
    println!("  • GPU:  {}{vram_str}", hw.gpu_name);
    println!("  • Disk: {:.1} GB free", hw.free_disk_gb);

    println!("\n{BOLD}Local Model Recommendation{RESET}");
    println!(
        "  • Recommended size:  {GREEN}{BOLD}{}{RESET}",
        rec.recommended_size
    );
    println!(
        "  • Suggested models:  {}",
        rec.recommended_models.join(", ")
    );
    println!(
        "  • Quantization:      {} (~{:.1} GB needed)",
        rec.recommended_quantization, rec.estimated_ram_required_gb
    );
    println!(
        "  • Fits in VRAM:      {}",
        if rec.fits_in_vram {
            "Yes (fully offloaded to GPU)"
        } else {
            "No (CPU/shared memory execution)"
        }
    );
    println!(
        "  • Trade-off:         {DIM}{}{RESET}",
        rec.tradeoff_explanation
    );
}

async fn run() -> Result<()> {
    enable_ansi_colors();
    let cli = Cli::parse();
    let home = config::home_dir();
    let config_path = cli
        .config
        .clone()
        .unwrap_or_else(config::default_config_path);
    let sessions_dir = home.join("sessions");
    let traces_dir = home.join("traces");

    if let Some(id) = &cli.trace {
        return trace::print_trace(&traces_dir, id);
    }

    if cli.init {
        return init_config(&config_path);
    }
    if cli.wizard {
        return wizard::run_cli_wizard(&config_path);
    }
    if let Some(action) = &cli.autostart {
        match action.to_ascii_lowercase().as_str() {
            "on" | "enable" | "true" => {
                let res = autostart::set_enabled(true)?;
                println!(
                    "Autostart enabled. Command: {}",
                    res.command.unwrap_or_default()
                );
            }
            "off" | "disable" | "false" => {
                autostart::set_enabled(false)?;
                println!("Autostart disabled.");
            }
            "status" => {
                let st = autostart::get_status()?;
                if st.enabled {
                    println!(
                        "Autostart is enabled (command: {}).",
                        st.command.unwrap_or_default()
                    );
                } else {
                    println!("Autostart is disabled.");
                }
            }
            other => {
                bail!("unknown autostart action '{other}'; use 'on', 'off', or 'status'");
            }
        }
        return Ok(());
    }
    if cli.hardware {
        print_hardware();
        return Ok(());
    }
    if let Some(p) = &cli.set_key {
        return keys::set(p);
    }
    if let Some(p) = &cli.delete_key {
        return keys::delete(p);
    }
    if let Some(url) = &cli.install_skill {
        let src = GithubSource::parse(url).map_err(anyhow::Error::msg)?;
        let dest = home.join("skills");
        println!(
            "Downloading {}/{}{} …",
            src.owner,
            src.repo,
            if src.path.is_empty() {
                String::new()
            } else {
                format!("/{}", src.path)
            }
        );
        let installed =
            oblivion_skills::install_from_github(&src, &dest).map_err(anyhow::Error::msg)?;
        for i in installed {
            println!(
                "  {GREEN}✓{RESET} {}  {DIM}{} · license: {}{RESET}",
                i.name,
                i.dir.display(),
                i.license
            );
        }
        println!("Skills load the next time Oblivion starts.");
        return Ok(());
    }

    let store = Arc::new(Store::open(&home.join("oblivion.db"))?);
    // Phase 0 kept sessions as JSON files; bring them into the database once.
    if sessions_dir.is_dir() {
        let _ = store.import_json_dir(&sessions_dir);
    }
    if cli.usage {
        let rows = store.usage_today()?;
        if rows.is_empty() {
            println!("No model requests yet today (UTC).");
        }
        for r in rows {
            println!(
                "  {:<45} {:>5} requests  {:>8} in / {:>7} out tokens",
                r.target, r.requests, r.prompt_tokens, r.completion_tokens
            );
        }
        return Ok(());
    }
    if cli.sessions {
        let list = store.list_sessions(30)?;
        if list.is_empty() {
            println!("No saved sessions yet.");
        }
        for s in list {
            println!(
                "{BOLD}{}{RESET}  {DIM}{}{RESET}  {}",
                s.id,
                s.workspace.display(),
                s.first_prompt
            );
        }
        return Ok(());
    }

    if let Some(mode) = &cli.setup_browser {
        return setup_browser(&config_path, mode.trim(), &home).await;
    }
    let cfg = Config::load(&config_path)?;
    cfg.validate()?;
    if !cfg.memory.embedding_model.is_empty() {
        let embedder = Arc::new(oblivion_store::OllamaEmbedder::new(
            &cfg.memory.ollama_endpoint,
            &cfg.memory.embedding_model,
        ));
        store.set_embedder(embedder);
    }
    let (targets, skipped) = cfg.targets(&keys::for_provider);

    if cli.models {
        println!("{BOLD}Models, in fallback order:{RESET}");
        for t in &targets {
            let used = store.requests_today(&t.label());
            let limits = match (t.requests_per_day, t.max_input_tokens) {
                (Some(d), Some(m)) => format!("{used}/{d} today · max ~{m} tokens/request"),
                (Some(d), None) => format!("{used}/{d} today"),
                (None, Some(m)) => format!("{used} today · max ~{m} tokens/request"),
                (None, None) => format!("{used} today"),
            };
            let env_var = cfg
                .providers
                .iter()
                .find(|p| p.name == t.provider.name)
                .and_then(|p| p.api_key_env.as_deref());
            let source = match keys::lookup(&t.provider.name, env_var) {
                Some((_, keys::KeySource::Env)) => {
                    "key: environment variable (overrides key store)"
                }
                Some((_, keys::KeySource::Store)) => "key: key store",
                None => "no key needed",
            };
            let checked_str = match &t.checked {
                Some(date) => format!(" · checked {date}"),
                None => String::new(),
            };
            println!(
                "  {GREEN}✓{RESET} {}  {DIM}{limits}{checked_str} · {source}{RESET}",
                t.label()
            );
        }
        for s in &skipped {
            println!("  {DIM}– {} skipped: {}{RESET}", s.name, s.reason);
        }
        return Ok(());
    }
    if targets.is_empty() {
        bail!(
            "no usable models. Save a key with `oblivion --set-key groq` (or openrouter, nvidia, …) or enable ollama in {}.\nSkipped: {}",
            config_path.display(),
            skipped.iter().map(|s| format!("{} ({})", s.name, s.reason)).collect::<Vec<_>>().join(", ")
        );
    }

    let rules = Rules::new(&cfg.permissions.allow, &cfg.permissions.deny)
        .map_err(|e| anyhow::anyhow!("[permissions] in {}: {e}", config_path.display()))?;
    let search = match cfg.search.backend.as_str() {
        "duckduckgo" => SearchBackend::DuckDuckGo,
        "tavily" => match keys::lookup("tavily", Some("TAVILY_API_KEY")) {
            Some((api_key, _)) => SearchBackend::Tavily { api_key },
            None => bail!("search backend is tavily but no key: run `oblivion --set-key tavily`"),
        },
        "searxng" => SearchBackend::SearxNg {
            base_url: cfg
                .search
                .searxng_url
                .clone()
                .context("search backend is searxng but searxng_url is not set")?,
        },
        other => bail!("unknown search backend '{other}' (duckduckgo, tavily, searxng)"),
    };

    let mut router = Router::new(targets).with_ledger(store.clone());
    if let Some(filter) = &cli.model {
        if !router.pin(filter) {
            bail!("no configured model matches '{filter}'. Run `oblivion --models` to see them.");
        }
    }

    let root = match &cli.cwd {
        Some(p) => {
            if !p.is_dir() {
                bail!("cannot open folder {}", p.display());
            }
            normalize_path(p)
        }
        None => normalize_path(&std::env::current_dir()?),
    };
    let shell = match &cfg.agent.shell {
        Some(s) => Shell::parse(s).with_context(|| format!("unknown shell '{s}' in config"))?,
        None => Shell::platform_default(),
    };
    let approval = if cli.yes {
        ApprovalMode::Yolo
    } else if let Some(a) = &cli.approval {
        ApprovalMode::parse(a)
            .with_context(|| format!("unknown approval mode '{a}' (ask, auto-edit, yolo)"))?
    } else {
        cfg.agent.approval
    };

    // Skills: bundled ones are refreshed on every start so updates ship with the binary.
    let skills = if cfg.skills.enabled {
        let _ = oblivion_skills::install_bundled(&home.join("bundled-skills"));
        let roots = SkillSet::standard_roots(&home, dirs_home().as_deref(), &root);
        Arc::new(SkillSet::discover(&roots))
    } else {
        Arc::new(SkillSet::default())
    };
    if cli.skills {
        if skills.is_empty() {
            println!("No skills found.");
        }
        for sk in skills.all() {
            let src = match sk.source {
                SkillSource::External => "shared",
                SkillSource::Bundled => "bundled",
                SkillSource::User => "user",
                SkillSource::Project => "project",
            };
            let d: String = sk.description.chars().take(90).collect();
            println!("  {BOLD}{}{RESET} {DIM}[{src}]{RESET} {d}", sk.name);
        }
        for w in &skills.warnings {
            println!("  {YELLOW}! {w}{RESET}");
        }
        return Ok(());
    }

    let mcp_servers = cfg
        .mcp_servers
        .iter()
        .filter(|m| m.enabled)
        .map(|m| oblivion_mcp::ServerConfig {
            name: m.name.clone(),
            description: m.description.clone(),
            transport: match (&m.command, &m.url) {
                (Some(cmd), _) => oblivion_mcp::Transport::Stdio {
                    command: cmd.clone(),
                    args: m.args.clone(),
                    env: m.env.clone(),
                },
                (None, Some(url)) => oblivion_mcp::Transport::Http {
                    url: url.clone(),
                    token: keys::lookup(&format!("mcp-{}", m.name), m.token_env.as_deref())
                        .map(|(t, _)| t),
                },
                (None, None) => unreachable!("validated in Config::validate"),
            },
            timeout: std::time::Duration::from_secs(m.timeout_secs),
        })
        .collect();
    let hub = Arc::new(McpHub::new(mcp_servers));
    if cli.mcp {
        if hub.is_empty() {
            println!(
                "No MCP servers configured. Add [[mcp_servers]] entries to {}.",
                config_path.display()
            );
        }
        for name in hub.names() {
            match hub.list_tools(&name).await {
                Ok(t) => println!("{GREEN}✓{RESET} {t}"),
                Err(e) => println!("{RED}✗ {name}: {e}{RESET}"),
            }
        }
        hub.shutdown().await;
        return Ok(());
    }

    let browser = cfg
        .browser
        .enabled
        .then(|| Arc::new(BrowserTool::new(&browser_settings(&cfg.browser, &home))));

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let sections = [
        skills.prompt_listing(cfg.skills.prompt_chars),
        hub.prompt_listing(),
        if browser.is_some() {
            BROWSER_PROMPT.to_string()
        } else {
            String::new()
        },
    ];
    let system = prompt::build(&root, shell, &today, &sections);
    // Sub-agents get the same environment and skills, but no MCP list (they cannot use MCP).
    let system_for_subagents = prompt::build(&root, shell, &today, &sections[..1]);
    let subagent_search = search.clone();
    let skill_dirs: Vec<PathBuf> = skills.all().map(|s| s.dir.clone()).collect();
    // The web page and full-screen UI answer approvals through the worker's channel.
    let interactive_ui = cli.web || cli.tui;
    let (ev_tx, ev_rx) = app::channels();
    let approvals = app::Approvals::new(ev_tx.clone());
    let base_approver: Box<dyn Approver> = if interactive_ui {
        approvals.approver()
    } else {
        Box::new(TerminalApprover)
    };
    let current_tracer: Arc<Mutex<Option<Arc<trace::Tracer>>>> = Arc::new(Mutex::new(None));
    let approver: Box<dyn Approver> = Box::new(TracingApprover {
        inner: base_approver,
        tracer: current_tracer.clone(),
    });
    let mut agent = {
        // Snapshot folder is named per run so undo data never mixes between sessions.
        let snap = home
            .join("snapshots")
            .join(chrono::Local::now().format("%Y%m%d-%H%M%S").to_string());
        let mut tools = ToolBox::new(root.clone(), shell, snap)
            .with_search(search)
            .allow_reads_in(skill_dirs.clone());
        if !skills.is_empty() {
            tools.add_provider(skills.clone());
        }
        if !hub.is_empty() {
            tools.add_provider(hub.clone());
        }
        if let Some(b) = &browser {
            tools.add_provider(b.clone());
        }
        let computer_settings = oblivion_tools::ComputerUseSettings {
            enabled: cfg.computer_use.enabled
                || std::env::var("OBLIVION_ENABLE_COMPUTER_USE").is_ok(),
            allowed_apps: cfg.computer_use.allowed_apps.clone(),
            ask_all: cfg.computer_use.ask_all,
            stop_hotkey: cfg.computer_use.stop_hotkey.clone(),
        };
        let computer_tool = Arc::new(oblivion_tools::ComputerTool::new(computer_settings));
        tools.add_provider(computer_tool);
        let memory_tools = Arc::new(oblivion_store::MemoryTools::with_project(
            store.clone(),
            root.to_string_lossy(),
        ));
        tools.add_provider(memory_tools);
        Agent::new(
            router,
            tools,
            approver,
            approval,
            cfg.agent.max_steps,
            system,
        )
    };
    agent.context_tokens = cfg.agent.context_tokens;
    agent.max_web_calls = cfg.agent.max_web_calls;
    agent.summary_prefer = cfg.routing.summary.clone();
    agent.offline = cfg.routing.offline || cli.offline;
    agent.routing_rules = cfg.routing.rules.clone();
    agent.small_model = cfg.agent.small_model || cli.small_model;
    agent.active_model = cli.model.clone();
    agent.small_system_prompt = Some(prompt::build_small(&root, shell, &today));
    agent.memory_auto_extract = cfg.memory.enabled && cfg.memory.auto_extract;
    agent.memory_extract_tokens = cfg.memory.max_tokens;
    agent.set_subagents(oblivion_core::SubagentFactory {
        router: agent.router.share(),
        root: std::sync::RwLock::new(root.clone()),
        shell,
        search: subagent_search,
        read_roots: skill_dirs.clone(),
        skills: if skills.is_empty() {
            None
        } else {
            Some(skills.clone() as Arc<dyn oblivion_tools::ToolProvider>)
        },
        base_prompt: system_for_subagents,
        context_tokens: cfg.agent.context_tokens,
        max_steps: cfg.routing.subagent_max_steps,
        rules: rules.clone(),
        prefer: cfg.routing.subagent.clone(),
    });
    agent.set_rules(rules);
    if let Some(id) = &cli.resume {
        agent.resume(store.load_session(id)?);
        println!(
            "{DIM}Resumed session {id} ({} messages){RESET}",
            agent.session.messages.len()
        );
    }
    agent.persist_with(store.clone());
    if cfg.memory.enabled {
        agent.set_memory_recall(store.clone());
    }

    if interactive_ui {
        let (worker, controls) = app::Worker::new(
            agent,
            skills.clone(),
            hub.clone(),
            browser.is_some(),
            ev_tx,
            approvals,
        );
        let worker = worker
            .with_traces(traces_dir, current_tracer.clone())
            .with_store(store.clone());
        let mut initial_images = if let Some(ref img_path) = cli.image {
            match commands::load_image_attachment(&img_path.to_string_lossy(), &root) {
                Ok(att) => vec![att],
                Err(e) => {
                    eprintln!("{RED}✗ Failed to load --image: {e:#}{RESET}");
                    vec![]
                }
            }
        } else {
            vec![]
        };
        if cli.screen {
            match screen::capture_screen() {
                Ok(att) => initial_images.push(att),
                Err(e) => eprintln!("{RED}✗ Failed to capture --screen: {e:#}{RESET}"),
            }
        }

        if !cli.prompt.is_empty() || !initial_images.is_empty() {
            let p = if cli.prompt.is_empty() {
                if cli.screen {
                    "Look at my screen and describe what you see.".to_string()
                } else {
                    "Describe this image.".to_string()
                }
            } else {
                cli.prompt.join(" ")
            };
            if !initial_images.is_empty() {
                controls.send_with_images(p, initial_images);
            } else {
                controls.send(p);
            }
        }
        let result = if cli.web {
            let opts = WebOptions {
                port: cli.port,
                open: !cli.no_open,
                offline: cli.offline,
                config_path: Some(config_path),
            };
            run_web(worker, controls, store.clone(), ev_rx, opts, &cfg).await
        } else {
            tui::run(worker, controls, ev_rx).await
        };
        hub.shutdown().await;
        if let Some(b) = &browser {
            b.shutdown().await;
        }
        return result;
    }
    drop((ev_tx, ev_rx, approvals));

    // Ctrl+C cancels the running request; at the prompt it exits.
    let in_turn = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(Notify::new());
    {
        let in_turn = in_turn.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            while tokio::signal::ctrl_c().await.is_ok() {
                if in_turn.load(Ordering::SeqCst) {
                    cancel.notify_one();
                } else {
                    println!();
                    std::process::exit(130);
                }
            }
        });
    }

    if !cli.prompt.is_empty() || cli.image.is_some() || cli.screen {
        let mut initial_images = if let Some(ref img_path) = cli.image {
            match commands::load_image_attachment(&img_path.to_string_lossy(), &root) {
                Ok(att) => vec![att],
                Err(e) => {
                    eprintln!("{RED}✗ Failed to load --image: {e:#}{RESET}");
                    vec![]
                }
            }
        } else {
            vec![]
        };
        if cli.screen {
            match screen::capture_screen() {
                Ok(att) => initial_images.push(att),
                Err(e) => eprintln!("{RED}✗ Failed to capture --screen: {e:#}{RESET}"),
            }
        }
        let mut task = if cli.prompt.is_empty() {
            if cli.screen {
                "Look at my screen and describe what you see.".to_string()
            } else {
                "Describe this image in detail.".to_string()
            }
        } else {
            cli.prompt.join(" ")
        };
        // `oblivion "/code-review the last commit"` runs a skill directly.
        if let Some(cmd) = task.strip_prefix('/') {
            let (name, arg) = cmd.split_once(' ').unwrap_or((cmd, ""));
            if skills.get(name).is_some() {
                task = skill_request(&skills, name, arg);
            }
        }
        run_one(
            &mut agent,
            &task,
            initial_images,
            &in_turn,
            &cancel,
            &traces_dir,
            current_tracer.clone(),
        )
        .await;
        extract_and_persist_memories(&agent, &store).await;
        hub.shutdown().await;
        if let Some(b) = &browser {
            b.shutdown().await;
        }
        return Ok(());
    }

    println!(
        "{BOLD}Oblivion{RESET} {DIM}v{} · {} · approval: {:?} · /help for commands{RESET}",
        env!("CARGO_PKG_VERSION"),
        root.display(),
        agent.approval
    );
    if let Some((name, _)) = prompt::project_instructions(&root) {
        println!("{DIM}Loaded project instructions from {name}{RESET}");
    }
    let stdin = std::io::stdin();
    loop {
        print!("\n{GREEN}❯{RESET} ");
        std::io::stdout().flush()?;
        let mut line = String::new();
        let n = tokio::task::block_in_place(|| stdin.lock().read_line(&mut line))?;
        if n == 0 {
            break; // EOF (Ctrl+D / Ctrl+Z)
        }
        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if let Some(cmd) = input.strip_prefix('/') {
            let mut sink = print_event;
            match handle_command(&mut agent, &skills, &hub, Some(&store), cmd, &mut sink).await? {
                Command::Quit => break,
                Command::Done(text) => println!("{text}"),
                Command::Run(task) => {
                    run_one(
                        &mut agent,
                        &task,
                        vec![],
                        &in_turn,
                        &cancel,
                        &traces_dir,
                        current_tracer.clone(),
                    )
                    .await
                }
                Command::RunWithImages(task, imgs) => {
                    run_one(
                        &mut agent,
                        &task,
                        imgs,
                        &in_turn,
                        &cancel,
                        &traces_dir,
                        current_tracer.clone(),
                    )
                    .await
                }
            }
            continue;
        }
        run_one(
            &mut agent,
            input,
            vec![],
            &in_turn,
            &cancel,
            &traces_dir,
            current_tracer.clone(),
        )
        .await;
    }
    extract_and_persist_memories(&agent, &store).await;
    hub.shutdown().await;
    if let Some(b) = &browser {
        b.shutdown().await;
    }
    Ok(())
}

async fn extract_and_persist_memories(agent: &Agent, store: &Arc<oblivion_store::Store>) {
    if agent.memory_auto_extract {
        let proposals = agent.extract_end_of_session_memories().await;
        if !proposals.is_empty() {
            println!(
                "\n{DIM}Learned {} memories from this session:{RESET}",
                proposals.len()
            );
            for p in &proposals {
                println!(
                    "  {DIM}• [{}] (importance {}): {}{RESET}",
                    p.mem_type, p.importance, p.text
                );
                let new_mem = oblivion_store::NewMemory::new(&p.mem_type, &p.text, p.importance)
                    .with_source_session(&agent.session.id);
                let _ = store.add_memory(&new_mem);
            }

            if let Some(proposal) = agent.propose_agents_md_update(&proposals) {
                let filename = proposal
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                println!("\n{BOLD}Project instructions update available for {filename}:{RESET}");
                for add in &proposal.additions {
                    println!("  {GREEN}+ {add}{RESET}");
                }
                let stdin = std::io::stdin();
                if stdin.is_terminal() {
                    print!("Update {filename}? [y/N]: ");
                    let _ = std::io::stdout().flush();
                    let mut ans = String::new();
                    if stdin.read_line(&mut ans).is_ok() {
                        let a = ans.trim().to_lowercase();
                        if a == "y" || a == "yes" {
                            match agent.apply_agents_md_update(&proposal) {
                                Ok(()) => println!("{GREEN}✓ Updated {filename}{RESET}"),
                                Err(e) => {
                                    eprintln!("{RED}✗ Failed to update {filename}: {e}{RESET}")
                                }
                            }
                        } else {
                            println!("{DIM}Skipped updating {filename}.{RESET}");
                        }
                    }
                } else {
                    println!(
                        "{DIM}Run /agents-md update to apply these additions to {filename}.{RESET}"
                    );
                }
            }
        }
    }
}

struct WebOptions {
    port: u16,
    open: bool,
    offline: bool,
    config_path: Option<PathBuf>,
}

/// `oblivion --web`: serve the page, then run the agent until Ctrl+C or /exit.
async fn run_web(
    worker: app::Worker,
    controls: app::Controls,
    store: Arc<oblivion_store::Store>,
    events: tokio::sync::mpsc::UnboundedReceiver<app::UiEvent>,
    opts: WebOptions,
    cfg: &Config,
) -> Result<()> {
    let bus = Arc::new(web::Bus::default());
    let token = web::new_token();
    let (listener, addr) = web::bind(opts.port)
        .await
        .context("could not open a local port for the web page")?;
    let url = format!("http://{addr}/#t={token}");
    let ollama = cfg
        .providers
        .iter()
        .find(|p| p.name == "ollama")
        .map(|p| Arc::new(oblivion_core::ollama::OllamaClient::new(&p.base_url)));
    let mut routing_cfg = cfg.routing.clone();
    if opts.offline {
        routing_cfg.offline = true;
    }
    let state = web::WebState {
        token: token.into(),
        controls: controls.clone(),
        bus: bus.clone(),
        store,
        ollama,
        routing: Arc::new(std::sync::RwLock::new(routing_cfg)),
        config_path: opts.config_path,
        voice_transcriber: None,
        voice_synthesizer: None,
        wake_word_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    tokio::spawn(web::pump(events, bus));
    tokio::spawn(async move {
        let _ = axum::serve(listener, web::router(state)).await;
    });
    println!("{BOLD}Oblivion web{RESET} is running. Open this link (it contains your access key):");
    println!("  {CYAN}{url}{RESET}");
    println!("{DIM}Press Ctrl+C here to stop.{RESET}");
    if opts.open {
        web::open_browser(&url);
    }
    // Ctrl+C: stop a running request first; a second press (or an idle one) quits.
    let ctrl_c = {
        let controls = controls.clone();
        async move {
            loop {
                if tokio::signal::ctrl_c().await.is_err() {
                    return;
                }
                if controls.busy() {
                    controls.cancel();
                    println!("{YELLOW}Stopping the running request (Ctrl+C again to quit).{RESET}");
                } else {
                    return;
                }
            }
        }
    };
    tokio::select! {
        _ = worker.run() => {}
        _ = ctrl_c => {}
    }
    // Release an approval that is still waiting, or the process could not exit.
    controls.cancel();
    println!("{DIM}Oblivion web stopped.{RESET}");
    Ok(())
}

const BROWSER_PROMPT: &str = "Web browser\nUse the `browser` tool for work on websites: logging in, clicking through \
pages, filling in forms, reading pages that need JavaScript or a login. For anything longer than a couple of \
steps, load the `browser` skill first. To simply look something up, web_search and web_fetch are faster and cheaper.\n";

fn browser_settings(b: &config::BrowserSection, home: &std::path::Path) -> BrowserSettings {
    BrowserSettings {
        mode: if b.mode == "extension" {
            BrowserMode::Extension
        } else {
            BrowserMode::Own
        },
        browser: b.browser.clone(),
        headless: b.headless,
        profile_dir: Some(home.join("browser-profile")),
        command: b.command.clone(),
        package: b.package.clone(),
        timeout: std::time::Duration::from_secs(b.timeout_secs),
    }
}

/// Run a program and return the first line of its output, if it ran.
fn program_version(program: &str) -> Option<String> {
    let out = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", program, "--version"])
            .output()
    } else {
        std::process::Command::new(program)
            .arg("--version")
            .output()
    }
    .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(|l| l.trim().to_string())
}

/// `oblivion --setup-browser [own|extension]`: check Node.js, switch the browser on
/// in the config file, start it once and show what the agent sees.
async fn setup_browser(config_path: &PathBuf, mode: &str, home: &std::path::Path) -> Result<()> {
    if !matches!(mode, "own" | "extension") {
        bail!("--setup-browser takes \"own\" (default) or \"extension\", not \"{mode}\"");
    }
    println!("{BOLD}Browser setup{RESET} ({mode} mode)");
    match program_version("node") {
        Some(v) => println!("  {GREEN}✓{RESET} Node.js {v}"),
        None => {
            println!(
                "  {RED}✗ Node.js was not found.{RESET} Install the LTS version from https://nodejs.org, \
                 open a NEW terminal, and run `oblivion --setup-browser` again."
            );
            return Ok(());
        }
    }
    if program_version("npx").is_none() {
        println!("  {RED}✗ npx was not found{RESET} (it comes with Node.js). Reinstall Node.js, open a new terminal and retry.");
        return Ok(());
    }

    if !config_path.exists() {
        init_config(config_path)?;
    }
    let text = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let updated = config::enable_browser_in(&text, mode);
    let cfg = Config::parse(&updated)?;
    cfg.validate()?;
    if updated != text {
        std::fs::write(config_path, &updated)?;
        println!(
            "  {GREEN}✓{RESET} [browser] turned on in {}",
            config_path.display()
        );
    } else {
        println!(
            "  {GREEN}✓{RESET} [browser] already on in {}",
            config_path.display()
        );
    }

    if mode == "extension" {
        println!(
            "  {YELLOW}!{RESET} Install the \"Playwright MCP Bridge\" extension in Chrome or Edge:\n    \
             https://chromewebstore.google.com/detail/playwright-mcp-bridge/mmlmfjhmonkocbjadbfplnigmagldckm\n    \
             When Oblivion connects, the browser shows a page asking which tab to share: approve it."
        );
    }

    println!(
        "  {DIM}Starting the browser and opening example.com (the first run downloads Playwright MCP; \
         this can take a minute)…{RESET}"
    );
    let browser = BrowserTool::new(&browser_settings(&cfg.browser, home));
    let result = browser.try_open("https://example.com").await;
    browser.shutdown().await;
    match result {
        Ok(page) => {
            println!("  {GREEN}✓{RESET} The browser works. The agent sees pages like this:");
            for l in page.lines().take(12) {
                println!("    {DIM}{l}{RESET}");
            }
        }
        Err(e) => {
            println!("  {RED}✗ The browser did not start:{RESET} {e}");
            println!(
                "    If it says a browser is missing, run: npx playwright install chrome\n    \
                 (or set browser = \"msedge\" under [browser] to use Edge, which Windows already has)."
            );
            return Ok(());
        }
    }

    // Page views are a few thousand tokens; Groq's free tier allows ~7K per request.
    let (targets, _) = cfg.targets(&keys::for_provider);
    if !targets.iter().any(roomy) {
        println!(
            "  {YELLOW}!{RESET} All your models have small request limits (Groq free tier: about 7K tokens), \
             and web pages are large.\n    Add a model with a big context so browser tasks work well, e.g. Gemini \
             (free key at https://aistudio.google.com/apikey):\n    oblivion --set-key gemini"
        );
    }
    println!(
        "Done. Start `oblivion` and ask, e.g.: open example.com and tell me what the page says"
    );
    Ok(())
}

/// Can this model take a request with a whole web page in it?
fn roomy(t: &Target) -> bool {
    t.max_input_tokens.is_none_or(|m| m >= 16_000)
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

async fn run_one(
    agent: &mut Agent,
    task: &str,
    images: Vec<oblivion_core::ImageAttachment>,
    in_turn: &AtomicBool,
    cancel: &Notify,
    traces_dir: &std::path::Path,
    tracer_slot: Arc<Mutex<Option<Arc<trace::Tracer>>>>,
) {
    in_turn.store(true, Ordering::SeqCst);
    let tracer = trace::Tracer::start(traces_dir, task, agent.tools.root()).map(Arc::new);
    if let Some(t) = &tracer {
        *tracer_slot.lock().unwrap() = Some(t.clone());
    }
    let tracer_ev = tracer.clone();
    let mut on_event = move |e: AgentEvent| {
        if let Some(t) = &tracer_ev {
            match &e {
                AgentEvent::Model(m) => t.record_model(m),
                AgentEvent::Fallback { from, reason } => t.record_fallback(from, reason),
                AgentEvent::ToolStart {
                    name,
                    summary,
                    detail,
                } => {
                    t.record_tool_start(name, summary, detail);
                }
                AgentEvent::ToolEnd {
                    name,
                    preview,
                    is_error,
                    ..
                } => {
                    t.record_tool_end(name, preview, *is_error);
                }
                AgentEvent::Declined { name } => t.record_declined(name),
                AgentEvent::Notice(msg) => t.record(trace::TraceEntry::Notice {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    text: msg.clone(),
                }),
                _ => {}
            }
        }
        print_event(e);
    };
    let result = tokio::select! {
        r = agent.run_turn_with_images(task, images, &mut on_event) => Some(r),
        _ = cancel.notified() => None,
    };
    in_turn.store(false, Ordering::SeqCst);
    match result {
        None => {
            if let Some(t) = &tracer {
                t.finish(0, 0, 0, 0, "cancelled", None);
            }
            agent.repair();
            println!(
                "\n{YELLOW}Cancelled. File changes so far are kept; /undo reverts them.{RESET}"
            );
        }
        Some(Err(e)) => {
            let err_msg = format!("{e:#}");
            if let Some(t) = &tracer {
                t.finish(0, 0, 0, 0, "error", Some(&err_msg));
            }
            println!("\n{RED}✗ {e:#}{RESET}");
        }
        Some(Ok(s)) => {
            if let Some(t) = &tracer {
                t.finish(
                    s.steps,
                    s.tool_calls,
                    s.usage.prompt_tokens,
                    s.usage.completion_tokens,
                    "done",
                    None,
                );
            }
            let tokens = if s.usage.prompt_tokens + s.usage.completion_tokens > 0 {
                format!(
                    " · {} in / {} out tokens",
                    s.usage.prompt_tokens, s.usage.completion_tokens
                )
            } else {
                String::new()
            };
            println!(
                "{DIM}  {} step(s) · {} tool call(s){tokens}{RESET}",
                s.steps, s.tool_calls
            );
        }
    }
    *tracer_slot.lock().unwrap() = None;
}
