//! `tctl` command-line front-end.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, CommandFactory, Parser, Subcommand};
use serde::Serialize;

use tctl::backend::{ProcessBackend, TartBackend};
use tctl::config::{self, Config};
use tctl::ops::{self, Ctx, Reporter};
use tctl::registry::Registry;
use tctl::{ENV_DEST, SHARE_MOUNT};

// ─── CLI definition ──────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "tctl",
    version,
    about = "Control Tart-based Debian dev VMs (per-project clones of the template image)",
    disable_help_subcommand = false,
    arg_required_else_help = true
)]
struct Cli {
    /// Machine-readable JSON output instead of text
    #[arg(long, global = true)]
    json: bool,
    /// Disable colours (also honours NO_COLOR)
    #[arg(long, global = true)]
    no_color: bool,
    /// Increase log verbosity (-v, -vv)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Suppress informational progress messages
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create .tart-ctl.toml in the current directory
    Init(InitArgs),
    /// Start the configured VM in the background
    #[command(visible_alias = "up")]
    Start {
        /// Seconds to wait for the VM to report an IP (0 = don't wait)
        #[arg(long, default_value_t = 60)]
        wait: u64,
    },
    /// Stop the configured VM
    #[command(visible_alias = "down")]
    Stop,
    /// Show VM state and current IP when running
    #[command(visible_alias = "st")]
    Status,
    /// Push merged env files into /etc/profile.d/tart-provision.sh inside the VM
    #[command(visible_alias = "pr")]
    Provision,
    /// Show the effective merged environment and optionally diff against the VM
    Env {
        /// Compare with the VM's current file (exit 1 when it differs)
        #[arg(long)]
        diff: bool,
        /// Do not mask secret-looking values
        #[arg(long)]
        show_secrets: bool,
    },
    /// SSH into the VM or run a single remote command (after `--`)
    #[command(visible_alias = "sh")]
    Ssh {
        /// Remote command and arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Print the VM IP if running
    Ip,
    /// Write or refresh the VM's Host block in the ssh client config
    #[command(visible_alias = "usc")]
    UpdateSshConfig,
    /// Inspect or rebuild the per-user VM registry
    Registry {
        #[command(subcommand)]
        cmd: RegistryCmd,
    },
    /// Explain config discovery and keys
    #[command(aliases = ["help-vars", "help-discovery"])]
    HelpConfig,
    /// Run the local HTTP/WebSocket control server in the foreground
    Server {
        /// TCP port on 127.0.0.1 (0 = pick an ephemeral port)
        #[arg(long, default_value_t = 7878)]
        port: u16,
        /// Exact browser origin allowed to call the API (repeatable)
        #[arg(long = "allow-origin", value_name = "ORIGIN")]
        allow_origin: Vec<String>,
    },
}

#[derive(Subcommand)]
enum RegistryCmd {
    /// List registered VMs
    List,
    /// Show one registered VM
    Show { name: String },
    /// Register every .tart-ctl.toml found below DIR
    Scan { dir: PathBuf },
    /// Remove a VM from the registry (does not touch Tart)
    Remove { name: String },
}

#[derive(Args)]
struct InitArgs {
    /// Name of the Tart VM this project uses
    #[arg(long)]
    vm: Option<String>,
    /// "auto" (this directory), "none", or a directory path
    #[arg(long)]
    share: Option<String>,
    /// Overwrite an existing file without asking
    #[arg(long)]
    force: bool,
}

// ─── Output helpers ──────────────────────────────────────────────────────────

struct Style {
    on: bool,
}
impl Style {
    fn wrap(&self, code: &str, s: &str) -> String {
        if self.on {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    fn bold(&self, s: &str) -> String {
        self.wrap("1", s)
    }
    fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }
    fn green(&self, s: &str) -> String {
        self.wrap("0;32", s)
    }
    fn yellow(&self, s: &str) -> String {
        self.wrap("0;33", s)
    }
    fn red(&self, s: &str) -> String {
        self.wrap("0;31", s)
    }
    fn cyan(&self, s: &str) -> String {
        self.wrap("0;36", s)
    }
    fn header(&self, s: &str) -> String {
        self.bold(&self.cyan(s))
    }
}

/// Prints messages immediately in the bash tool's format.
struct Console<'a> {
    style: &'a Style,
    quiet: bool,
}
impl Reporter for Console<'_> {
    fn info(&mut self, m: &str) {
        if !self.quiet {
            println!("{} {m}", self.style.green("==>"));
        }
    }
    fn warn(&mut self, m: &str) {
        eprintln!("{} {m}", self.style.yellow("[warn]"));
    }
    fn dim(&mut self, m: &str) {
        if !self.quiet {
            println!("{}", self.style.dim(m));
        }
    }
}

fn print_error(style: &Style, msg: &str) {
    eprintln!("{} {msg}", style.red("[error]"));
}

/// `==>` progress line, suppressed by `--quiet` (same rules as [`Console`]).
fn console_note(style: &Style, quiet: bool, msg: &str) {
    if !quiet {
        println!("{} {msg}", style.green("==>"));
    }
}

fn emit_json<T: Serialize>(value: &T, messages: &ops::Collected) -> anyhow::Result<()> {
    #[derive(Serialize)]
    struct Envelope<'a, T: Serialize> {
        ok: bool,
        result: &'a T,
        messages: &'a [(String, String)],
    }
    let out = Envelope {
        ok: true,
        result: value,
        messages: &messages.messages,
    };
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn emit_json_error(err: &dyn std::fmt::Display) {
    let v = serde_json::json!({ "ok": false, "error": err.to_string() });
    println!("{}", serde_json::to_string_pretty(&v).unwrap());
}

// ─── main ────────────────────────────────────────────────────────────────────

fn main() -> ExitCode {
    if std::env::args_os().len() == 1 {
        let _ = Cli::command().print_help();
        println!();
        return ExitCode::SUCCESS;
    }
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            // clap prints help/version itself; usage errors exit 1 like the bash tool did.
            let is_help = matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            );
            let json = std::env::args()
                .skip(1)
                .take_while(|a| a != "--")
                .any(|a| a == "--json");
            if json && !is_help {
                emit_json_error(&e);
            } else {
                let _ = e.print();
            }
            return if is_help {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            };
        }
    };

    let level = if cli.quiet {
        "error"
    } else {
        match cli.verbose {
            0 => "warn",
            1 => "info",
            _ => "debug",
        }
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| level.into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(
            !cli.no_color
                && !cli.json
                && std::env::var_os("NO_COLOR").is_none()
                && std::io::stderr().is_terminal(),
        )
        .without_time()
        .init();

    let style = Style {
        on: !cli.no_color
            && !cli.json
            && std::env::var_os("NO_COLOR").is_none()
            && std::io::stdout().is_terminal(),
    };

    match run(&cli, &style) {
        Ok(code) => code,
        Err(e) => {
            if cli.json {
                emit_json_error(&e);
            } else {
                print_error(&style, &e.to_string());
            }
            ExitCode::from(1)
        }
    }
}

fn run(cli: &Cli, style: &Style) -> anyhow::Result<ExitCode> {
    let cwd = std::env::current_dir()?;

    // Commands that need no project config.
    match &cli.cmd {
        Cmd::Init(args) => return cmd_init(args, &cwd, style, cli.json),
        Cmd::HelpConfig => {
            if cli.json {
                emit_json(
                    &serde_json::json!({ "text": help_config(style) }),
                    &ops::Collected::default(),
                )?;
            } else {
                print!("{}", help_config(style));
            }
            return Ok(ExitCode::SUCCESS);
        }
        Cmd::Server { port, allow_origin } => {
            for origin in allow_origin {
                if let Err(e) = tctl::server::validate_origin(origin) {
                    anyhow::bail!(e);
                }
            }
            eprintln!(
                "{} No authentication: every local process can control your VMs through this server.",
                style.yellow("[warn]")
            );
            tctl::server::run(tctl::server::ServerOptions {
                port: *port,
                allowed_origins: allow_origin.clone(),
            })?;
            return Ok(ExitCode::SUCCESS);
        }
        Cmd::Registry { cmd } => return cmd_registry(cmd, style, cli.json),
        _ => {}
    }

    let cfg = Config::load(&cwd)?;
    let backend = ProcessBackend;
    let registry = Registry::open_default().ok();
    if registry.is_none() {
        tracing::warn!("registry unavailable; continuing without it");
    }

    if !cli.json && !cli.quiet {
        if !cfg.global_exists {
            eprintln!(
                "{} No global config at '{}' — run 'just install' or copy config_template/config.toml there.",
                style.yellow("[warn]"),
                cfg.global_path.display()
            );
        }
        println!(
            "{}",
            style.dim(&format!("Global : {}", cfg.global_path.display()))
        );
        println!(
            "{}",
            style.dim(&format!("Project: {}", cfg.project_path.display()))
        );
        println!("{}", style.dim(&format!("VM     : {}", cfg.vm_name)));
        println!(
            "{}",
            style.dim(&format!("Share  : {}", share_label(&cfg.share_dir)))
        );
        println!();
    }

    let mut console = Console {
        style,
        quiet: cli.quiet,
    };
    let mut collected = ops::Collected::default();
    let reporter: &mut dyn Reporter = if cli.json {
        &mut collected
    } else {
        &mut console
    };
    let mut ctx = Ctx {
        cfg: &cfg,
        backend: &backend,
        registry: registry.as_ref(),
        reporter,
    };

    match &cli.cmd {
        Cmd::Start { wait } => {
            let r = ops::start(&mut ctx, *wait)?;
            if cli.json {
                emit_json(&r, &collected)?;
            }
        }
        Cmd::Stop => {
            let r = ops::stop(&mut ctx)?;
            if cli.json {
                emit_json(&r, &collected)?;
            }
        }
        Cmd::Status => {
            let r = ops::status(&mut ctx)?;
            if cli.json {
                emit_json(&r, &collected)?;
            } else {
                println!("{}", style.header(&format!("Status: {}", r.vm)));
                match r.state.as_str() {
                    "running" => {
                        println!("  State : {}", style.green("running"));
                        println!("  IP    : {}", r.ip.as_deref().unwrap_or("unknown"));
                    }
                    "stopped" => println!("  State : {}", style.yellow("stopped")),
                    "unknown" => println!(
                        "  State : {} (VM not found in tart list)",
                        style.red("unknown")
                    ),
                    other => println!("  State : {}", style.dim(other)),
                }
                println!("  Global : {}", r.global_config.display());
                println!("  Project: {}", r.project_config.display());
                println!("  Share  : {}", share_label(&r.share_dir));
            }
        }
        Cmd::Ip => {
            let r = ops::ip(&mut ctx)?;
            if cli.json {
                emit_json(&r, &collected)?;
            } else {
                println!("{}", r.ip);
            }
        }
        Cmd::UpdateSshConfig => {
            let r = ops::update_ssh_config(&mut ctx)?;
            if cli.json {
                emit_json(&r, &collected)?;
            }
        }
        Cmd::Provision => {
            let r = ops::provision(&mut ctx)?;
            if cli.json {
                emit_json(&r, &collected)?;
            }
        }
        Cmd::Env { diff, show_secrets } => {
            let mut r = ops::env(&mut ctx, *show_secrets)?;
            if !cli.json {
                ctx.reporter.info("Env layers (later override earlier):");
                for f in &r.layers {
                    ctx.reporter.dim(&format!("  + {}", f.display()));
                }
                println!();
                println!("{}", style.header("Effective environment"));
                if r.effective.is_empty() {
                    println!("  (empty)");
                }
                for e in &r.effective {
                    println!(
                        "  {:<24} = {:<32}  # {}",
                        e.key,
                        e.value,
                        e.source.display()
                    );
                }
                println!();
                if r.masked {
                    ctx.reporter
                        .dim("Secret-looking values are masked; use --show-secrets to reveal.");
                }
            }
            let mut failed = false;
            if *diff {
                if !cli.json {
                    println!();
                }
                let d = ops::env_diff(&mut ctx, &mut r, *show_secrets)?;
                failed = !d.up_to_date;
                if !cli.json {
                    println!(
                        "{}",
                        style.header(&format!("Diff: VM {ENV_DEST} → local layers"))
                    );
                    if d.up_to_date {
                        console_note(style, cli.quiet, "VM is up to date.");
                    } else {
                        print!("{}", d.text);
                        std::io::stdout().flush()?;
                        eprintln!(
                            "{} VM differs — run 'tctl provision' to update.",
                            style.yellow("[warn]")
                        );
                    }
                }
            }
            if cli.json {
                emit_json(&r, &collected)?;
            }
            if failed {
                return Ok(ExitCode::from(1));
            }
        }
        Cmd::Ssh { command } => {
            let target = ops::prepare_ssh(&mut ctx)?;
            let remote: Vec<String> = command
                .iter()
                .skip_while(|a| a.as_str() == "--")
                .cloned()
                .collect();
            std::io::stdout().flush()?;
            backend.exec_ssh(&target.settings, &target.host, &remote)?;
        }
        Cmd::Init(_) | Cmd::HelpConfig | Cmd::Server { .. } | Cmd::Registry { .. } => {
            unreachable!()
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn share_label(dir: &Option<PathBuf>) -> String {
    dir.as_ref()
        .map(|d| d.display().to_string())
        .unwrap_or_else(|| "(none)".to_string())
}

// ─── init ────────────────────────────────────────────────────────────────────

fn prompt(question: &str) -> anyhow::Result<String> {
    print!("{question}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn cmd_init(args: &InitArgs, cwd: &Path, style: &Style, json: bool) -> anyhow::Result<ExitCode> {
    let target = cwd.join(config::PROJECT_FILE);
    let interactive = std::io::stdin().is_terminal() && !json;

    if !json {
        println!(
            "{}",
            style.header(&format!(
                "Initialise {} in {}",
                config::PROJECT_FILE,
                cwd.display()
            ))
        );
        println!();
    }
    if target.exists() && !args.force {
        if !interactive {
            anyhow::bail!(
                "'{}' already exists (use --force to overwrite).",
                target.display()
            );
        }
        let a = prompt(&format!(
            "{} '{}' already exists. Overwrite? [y/N] ",
            style.yellow("[warn]"),
            target.display()
        ))?;
        if !a.eq_ignore_ascii_case("y") {
            anyhow::bail!("Aborted — existing file left untouched.");
        }
    }

    let global_path = config::global_config_path();
    let (global, _) = config::load_global_config()?;

    // Known local VMs (best effort).
    let vms: Vec<String> = ProcessBackend
        .list()
        .map(|l| {
            l.into_iter()
                .filter(|v| v.source == "local")
                .map(|v| v.name)
                .collect()
        })
        .unwrap_or_default();
    if !json && !vms.is_empty() {
        println!(
            "{}",
            style.dim(&format!("Local Tart VMs: {}", vms.join(" ")))
        );
    }

    let vm_name = match &args.vm {
        Some(v) if config::valid_vm_name(v) => v.clone(),
        Some(v) => {
            anyhow::bail!("Invalid VM name '{v}': use only letters, digits, '.', '_' and '-'.")
        }
        None if !interactive => anyhow::bail!("--vm is required when not running interactively."),
        None => loop {
            let v: String = prompt("VM name to manage from this project: ")?
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            if config::valid_vm_name(&v) {
                break v;
            }
            print_error(style, "Use only letters, digits, '.', '_' and '-'.");
        },
    };
    if !json && !vms.is_empty() && !vms.iter().any(|v| v == &vm_name) {
        eprintln!(
            "{} No local Tart VM named '{vm_name}' exists yet.",
            style.yellow("[warn]")
        );
        println!("{}", style.dim(&format!("  Create it by cloning the template:  just clone {vm_name}   (= tart clone debian-ssh {vm_name})")));
    }

    let global_share = global
        .share
        .as_ref()
        .and_then(|s| s.dir.clone())
        .unwrap_or_default();
    let default_choice = match global_share.as_str() {
        "auto" => "1",
        "" | "none" => "3",
        _ => "2",
    };
    let share_value: String = match &args.share {
        Some(s) => normalise_share(s, cwd)?,
        None if !interactive => {
            anyhow::bail!("--share is required when not running interactively (auto|none|<dir>).")
        }
        None => {
            println!();
            println!("Host directory to mount in the VM at {SHARE_MOUNT}:");
            println!("  1) auto  — this directory ({})", cwd.display());
            println!("  2) path  — a specific directory");
            println!("  3) none  — start the VM without a share");
            let shown = if global_share.is_empty() {
                "none"
            } else {
                &global_share
            };
            println!(
                "{}",
                style.dim(&format!(
                    "  (global config currently: {shown}; Enter keeps that → {default_choice})"
                ))
            );
            loop {
                let a = prompt(&format!("Choice [{default_choice}]: "))?;
                let a = if a.is_empty() {
                    default_choice
                } else {
                    a.as_str()
                };
                match a {
                    "1" | "auto" => break "auto".to_string(),
                    "3" | "none" => break "none".to_string(),
                    "2" | "path" => {
                        let d = prompt("Directory to share: ")?;
                        match normalise_share(&d, cwd) {
                            Ok(v) => break v,
                            Err(e) => print_error(style, &e.to_string()),
                        }
                    }
                    _ => print_error(style, "Please answer 1, 2 or 3."),
                }
            }
        }
    };

    let today = today_utc();
    let mut text = format!(
        "# tctl project marker — generated by tctl init on {today}\n# Global defaults: {} (this file overrides them for this project).\n\n[vm]\n# Tart VM managed from this project (clone of the template: just clone {vm_name})\nname = \"{vm_name}\"\n\n[share]\n# Host directory mounted in the VM at {SHARE_MOUNT}: \"auto\" = the directory containing this file,\n# \"none\" = start without a share, or an absolute path.\ndir = {}\n\n[provision]\n# Env files pushed by `tctl provision`, layered in order (relative to this directory).\n# env_files = [\".env\", \".env.local\"]\n",
        global_path.display(),
        toml_string(&share_value),
    );
    if !text.ends_with('\n') {
        text.push('\n');
    }
    std::fs::write(&target, text)?;

    if let Ok(reg) = Registry::open_default() {
        if let Ok(cfg) = Config::load(cwd) {
            let _ = reg.upsert_config(&cfg);
        }
    }

    if json {
        emit_json(
            &serde_json::json!({ "file": target, "vm": vm_name, "share": share_value }),
            &ops::Collected::default(),
        )?;
    } else {
        println!();
        println!("{} Wrote {}", style.green("==>"), target.display());
        println!(
            "{}",
            style.dim("Next: tctl up  →  tctl update-ssh-config  →  tctl ssh")
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn normalise_share(value: &str, cwd: &Path) -> anyhow::Result<String> {
    match value.trim() {
        "auto" | "1" => Ok("auto".into()),
        "none" | "" | "3" => Ok("none".into()),
        p => {
            let path = config::expand_home(p);
            let path = if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            };
            if !path.is_dir() {
                anyhow::bail!("'{}' is not a directory.", path.display());
            }
            Ok(path.canonicalize()?.display().to_string())
        }
    }
}

fn toml_string(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// Today's UTC date as `YYYY-MM-DD`, without a chrono dependency.
fn today_utc() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Gregorian date for a day count since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

// ─── registry ────────────────────────────────────────────────────────────────

fn cmd_registry(cmd: &RegistryCmd, style: &Style, json: bool) -> anyhow::Result<ExitCode> {
    let reg = Registry::open_default()?;
    match cmd {
        RegistryCmd::List => {
            let rows = reg.list()?;
            if json {
                emit_json(&rows, &ops::Collected::default())?;
            } else {
                println!(
                    "{}",
                    style.header(&format!("Registered VMs ({})", reg.path.display()))
                );
                if rows.is_empty() {
                    println!(
                        "  (empty) — run 'tctl registry scan <dir>' or 'tctl init' in a project"
                    );
                }
                for r in rows {
                    println!(
                        "  {:<24} {:<16} {}",
                        r.name,
                        r.last_ip.as_deref().unwrap_or("-"),
                        r.project_dir.display()
                    );
                }
            }
        }
        RegistryCmd::Show { name } => match reg.get(name)? {
            Some(r) => {
                if json {
                    emit_json(&r, &ops::Collected::default())?;
                } else {
                    println!("{}", serde_json::to_string_pretty(&r)?);
                }
            }
            None => anyhow::bail!("'{name}' is not registered."),
        },
        RegistryCmd::Scan { dir } => {
            let report = reg.scan(dir)?;
            let (found, failed) = (&report.registered, &report.failed);
            if json {
                emit_json(&report, &ops::Collected::default())?;
            } else {
                for n in found {
                    println!("{} registered {n}", style.green("==>"));
                }
                for (f, e) in failed {
                    eprintln!("{} {}: {e}", style.yellow("[warn]"), f.display());
                }
                println!(
                    "{}",
                    style.dim(&format!(
                        "{} VM(s) registered, {} file(s) skipped",
                        found.len(),
                        failed.len()
                    ))
                );
            }
        }
        RegistryCmd::Remove { name } => {
            if !reg.remove(name)? {
                anyhow::bail!("'{name}' is not registered.");
            }
            if json {
                emit_json(
                    &serde_json::json!({ "removed": name }),
                    &ops::Collected::default(),
                )?;
            } else {
                println!("{} removed {name}", style.green("==>"));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

// ─── help-config ─────────────────────────────────────────────────────────────

fn help_config(style: &Style) -> String {
    let global = config::global_config_path();
    format!(
        "\n{}\n\n{}\n    Config is layered (later overrides earlier, per key):\n      1) {}  — global defaults\n      2) {}       — project marker found by walking upward from PWD to '/'\n    The directory containing {} becomes the project root for relative paths.\n\n{}\n    [vm]        name            VM name to manage (project file only).\n    [ssh]       user            SSH username inside the VM.\n                config_path     SSH client config file maintained by update-ssh-config.\n                identity_file   SSH private key used for VM access.\n\n{}\n    [share]     dir             \"auto\" (project root), \"none\", or a host path mounted at {}.\n    [ssh]       remote_forward  List of \"PORT HOST:PORT\" RemoteForward entries (VM → host).\n                local_forward   List of \"PORT HOST:PORT\" LocalForward entries (host → VM).\n    [provision] env_files       List of env files (default [\".env\"]); relative entries resolve\n                                from the project root, later files override earlier keys.\n\n",
        style.header("tctl — config discovery and keys"),
        style.bold("DISCOVERY"),
        global.display(),
        config::PROJECT_FILE,
        config::PROJECT_FILE,
        style.bold("REQUIRED"),
        style.bold("OPTIONAL"),
        SHARE_MOUNT,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn civil_date_matches_known_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }
}
