//! `oblivion` command-line interface (SRS FR-70/71).

mod keys;

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::Parser;
use oblivion_core::config::{self, Config, DEFAULT_CONFIG};
use oblivion_core::{prompt, Agent, AgentEvent, ApprovalMode, Approver, Decision, Rules};
use oblivion_mcp::McpHub;
use oblivion_providers::{Router, UsageLedger};
use oblivion_skills::{GithubSource, SkillSet, SkillSource};
use oblivion_store::Store;
use oblivion_tools::{normalize_path, SearchBackend, Shell, TodoList, ToolBox};
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

fn print_event(e: AgentEvent) {
    let mut out = std::io::stdout();
    match e {
        AgentEvent::Text(t) => {
            let _ = write!(out, "{t}");
        }
        AgentEvent::TextEnd => {
            let _ = writeln!(out);
        }
        AgentEvent::ToolStart { name, summary } => {
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

async fn run() -> Result<()> {
    enable_ansi_colors();
    let cli = Cli::parse();
    let home = config::home_dir();
    let config_path = cli
        .config
        .clone()
        .unwrap_or_else(config::default_config_path);
    let sessions_dir = home.join("sessions");

    if cli.init {
        return init_config(&config_path);
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

    let cfg = Config::load(&config_path)?;
    cfg.validate()?;
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
            println!(
                "  {GREEN}✓{RESET} {}  {DIM}{limits} · {source}{RESET}",
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

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let sections = [
        skills.prompt_listing(cfg.skills.prompt_chars),
        hub.prompt_listing(),
    ];
    let system = prompt::build(&root, shell, &today, &sections);
    // Sub-agents get the same environment and skills, but no MCP list (they cannot use MCP).
    let system_for_subagents = prompt::build(&root, shell, &today, &sections[..1]);
    let subagent_search = search.clone();
    let skill_dirs: Vec<PathBuf> = skills.all().map(|s| s.dir.clone()).collect();
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
        Agent::new(
            router,
            tools,
            Box::new(TerminalApprover),
            approval,
            cfg.agent.max_steps,
            system,
        )
    };
    agent.context_tokens = cfg.agent.context_tokens;
    agent.max_web_calls = cfg.agent.max_web_calls;
    agent.summary_prefer = cfg.routing.summary.clone();
    agent.set_subagents(oblivion_core::SubagentFactory {
        router: agent.router.share(),
        root: root.clone(),
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

    if !cli.prompt.is_empty() {
        let mut task = cli.prompt.join(" ");
        // `oblivion "/code-review the last commit"` runs a skill directly.
        if let Some(cmd) = task.strip_prefix('/') {
            let (name, arg) = cmd.split_once(' ').unwrap_or((cmd, ""));
            if skills.get(name).is_some() {
                task = skill_request(&skills, name, arg);
            }
        }
        run_one(&mut agent, &task, &in_turn, &cancel).await;
        hub.shutdown().await;
        return Ok(());
    }

    println!(
        "{BOLD}Oblivion{RESET} {DIM}v{} · {} · approval: {:?} · /help for commands{RESET}",
        env!("CARGO_PKG_VERSION"),
        root.display(),
        agent.approval
    );
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
            match handle_command(&mut agent, &skills, &hub, cmd).await? {
                Command::Quit => break,
                Command::Done => {}
                Command::Run(task) => run_one(&mut agent, &task, &in_turn, &cancel).await,
            }
            continue;
        }
        run_one(&mut agent, input, &in_turn, &cancel).await;
    }
    hub.shutdown().await;
    Ok(())
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// The user message for `/skill-name args`: the skill's instructions are
/// included directly, saving the model a round trip to load them.
fn skill_request(skills: &SkillSet, name: &str, arg: &str) -> String {
    let loaded = skills.load_text(name).unwrap_or_default();
    let request = if arg.trim().is_empty() {
        "Follow the skill's instructions."
    } else {
        arg.trim()
    };
    format!("The user ran the /{name} skill. Its instructions are loaded below; follow them.\n\n{loaded}\n\nRequest: {request}")
}

enum Command {
    Quit,
    Done,
    Run(String),
}

async fn run_one(agent: &mut Agent, task: &str, in_turn: &AtomicBool, cancel: &Notify) {
    in_turn.store(true, Ordering::SeqCst);
    let mut on_event = print_event;
    let result = tokio::select! {
        r = agent.run_turn(task, &mut on_event) => Some(r),
        _ = cancel.notified() => None,
    };
    in_turn.store(false, Ordering::SeqCst);
    match result {
        None => {
            agent.repair();
            println!(
                "\n{YELLOW}Cancelled. File changes so far are kept; /undo reverts them.{RESET}"
            );
        }
        Some(Err(e)) => println!("\n{RED}✗ {e:#}{RESET}"),
        Some(Ok(s)) => {
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
}

/// Returns true when the user asked to quit.
async fn handle_command(
    agent: &mut Agent,
    skills: &SkillSet,
    hub: &McpHub,
    cmd: &str,
) -> Result<Command> {
    let (name, arg) = cmd
        .split_once(' ')
        .map(|(a, b)| (a, b.trim()))
        .unwrap_or((cmd, ""));
    match name {
        "exit" | "quit" | "q" => return Ok(Command::Quit),
        name if skills.get(name).is_some() => {
            return Ok(Command::Run(skill_request(skills, name, arg)));
        }
        "skills" => {
            for sk in skills.all() {
                let d: String = sk.description.chars().take(80).collect();
                println!("  /{:<16} {DIM}{d}{RESET}", sk.name);
            }
            if skills.is_empty() {
                println!("No skills found.");
            }
        }
        "mcp" => {
            if hub.is_empty() {
                println!("No MCP servers configured.");
            }
            for name in hub.names() {
                match hub.list_tools(&name).await {
                    Ok(t) => println!("{t}"),
                    Err(e) => println!("{RED}✗ {name}: {e}{RESET}"),
                }
            }
        }
        "help" | "h" => {
            println!("/undo          revert file changes from the last request");
            println!("/models        show models, cooldowns and today's usage");
            println!("/todo          show the agent's task list");
            println!("/compact       summarize older turns now to free up space");
            println!("/model <text>  only use models whose label contains <text>");
            println!("/approval <m>  ask | auto-edit | yolo");
            println!("/clear         start a fresh conversation");
            println!("/session       show the session id (resume with --resume <id>)");
            println!("/skills        list skills; run one with /<skill-name> [request]");
            println!("/mcp           list MCP servers and their tools");
            println!("/exit          quit (Ctrl+C also quits at the prompt)");
        }
        "undo" => match agent.undo_last_turn()? {
            Some(r) => println!(
                "Restored {} file(s), removed {} new file(s).",
                r.restored, r.removed
            ),
            None => println!("Nothing to undo."),
        },
        "models" => {
            for st in agent.router.status() {
                let used = match (st.used_today, st.requests_per_day) {
                    (Some(u), Some(d)) => format!("{u}/{d} today"),
                    (Some(u), None) => format!("{u} today"),
                    _ => String::new(),
                };
                match st.cooldown {
                    Some(d) => println!(
                        "  {YELLOW}…{RESET} {} {DIM}(cooling down {}s) {used}{RESET}",
                        st.label,
                        d.as_secs()
                    ),
                    None => println!("  {GREEN}✓{RESET} {} {DIM}{used}{RESET}", st.label),
                }
            }
        }
        "todo" => println!("{}", TodoList::render(&agent.tools.todos().items())),
        "compact" => {
            let mut sink = print_event;
            if agent.compact(true, &mut sink).await? {
                println!("Earlier turns summarized.");
            } else {
                println!("Nothing to compact yet.");
            }
        }
        "model" => {
            if arg.is_empty() {
                println!("Usage: /model <text>");
            } else if agent.router.pin(arg) {
                println!(
                    "Using: {}",
                    agent
                        .router
                        .targets()
                        .iter()
                        .map(|t| t.label())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            } else {
                println!("No model matches '{arg}'.");
            }
        }
        "approval" => match ApprovalMode::parse(arg) {
            Some(m) => {
                agent.approval = m;
                println!("Approval mode: {m:?}");
            }
            None => println!("Usage: /approval ask | auto-edit | yolo"),
        },
        "clear" => {
            agent.clear();
            println!("Conversation cleared.");
        }
        "session" => println!("Session {}", agent.session.id),
        other => println!("Unknown command /{other}. Try /help or /skills."),
    }
    Ok(Command::Done)
}
