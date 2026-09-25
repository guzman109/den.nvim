//! `den`: Den from the shell.
//!
//! ```text
//! den prompt            one short line for the shell prompt
//! den capture <text>    save a thought to this folder's project
//! den status            what Den knows about this folder
//! den tasks [--all]     open tasks as plain text
//! den stop              stop the timer
//! ```
//!
//! `den prompt` runs before every shell prompt, so it reads only what it
//! needs (the project files and this machine's timer log) and never prints an
//! error: a broken prompt is worse than an empty one.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use den_core::query::{Insight, Scope, TaskRow};
use den_core::timer::TimerLog;
use den_core::{Config, Kind, State, Vault};
use jiff::Timestamp;
use jiff::civil::Date;

#[derive(Parser)]
#[command(name = "den", version, about = "Den from the shell.")]
struct Cli {
    /// The vault folder. Defaults to the one in ~/.config/den/config.yaml.
    #[arg(long, global = true)]
    vault: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// One short line for the shell prompt: the timer and the most urgent
    /// thing about this folder's project.
    Prompt {
        /// No symbols, only words and numbers.
        #[arg(long)]
        plain: bool,
    },
    /// Save a quick thought to this folder's project, or to the general inbox.
    Capture {
        /// A project name instead of the one for this folder.
        #[arg(long)]
        to: Option<String>,
        #[arg(required = true)]
        text: Vec<String>,
    },
    /// What Den knows about this folder.
    Status,
    /// Open tasks, as plain text.
    Tasks {
        /// Every project, not just this folder's.
        #[arg(long)]
        all: bool,
    },
    /// Stop the timer.
    Stop,
}

type Result<T> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Command::Prompt { plain } = cli.command {
        if let Ok(line) = prompt(cli.vault.as_deref(), plain)
            && !line.is_empty()
        {
            println!("{line}");
        }
        return ExitCode::SUCCESS;
    }
    let result = match cli.command {
        Command::Prompt { .. } => Ok(()),
        Command::Capture { to, text } => capture(cli.vault.as_deref(), to, &text.join(" ")),
        Command::Status => status(cli.vault.as_deref()),
        Command::Tasks { all } => tasks(cli.vault.as_deref(), all),
        Command::Stop => stop(cli.vault.as_deref()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("den: {e}");
            ExitCode::FAILURE
        }
    }
}

fn setup(vault: Option<&Path>) -> Result<(Config, PathBuf)> {
    let config = Config::load().map_err(|e| e.to_string())?;
    let root = vault.map_or_else(|| config.vault_root(), Path::to_path_buf);
    Ok((config, root))
}

fn today() -> Date {
    jiff::Zoned::now().date()
}

fn cwd() -> Result<PathBuf> {
    std::env::current_dir().map_err(|e| e.to_string())
}

fn clock(seconds: i64) -> String {
    let s = seconds.max(0);
    let (h, m, s) = (s / 3600, s % 3600 / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

fn relative(date: Date, today: Date) -> String {
    let days = den_core::query::days_between(today, date);
    match days {
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        -1 => "yesterday".to_string(),
        2..=7 => format!("in {days} days"),
        -7..=-2 => format!("{} days ago", -days),
        _ => date.to_string(),
    }
}

fn prompt(vault: Option<&Path>, plain: bool) -> Result<String> {
    let (config, root) = setup(vault)?;
    let mut parts = Vec::new();
    let log = TimerLog::load_own(&root, &config.machine_name()).map_err(|e| e.to_string())?;
    if let Some(r) = log.running() {
        let secs = Timestamp::now().as_second() - r.since.as_second();
        parts.push(if plain {
            clock(secs)
        } else {
            format!("◐ {}", clock(secs))
        });
    }
    let vault = Vault::open_only(&root, Some(&[Kind::Project])).map_err(|e| e.to_string())?;
    if let Some(project) = vault.project_for_dir(&cwd()?) {
        let scope = Scope::Project(project.name().to_string());
        if let Some(first) = vault.insights(&scope, today()).first() {
            parts.push(match first {
                Insight::Overdue { count, .. } => format!("{count} overdue"),
                Insight::DueToday { .. } => "due today".to_string(),
                Insight::BehindPace { .. } => "behind pace".to_string(),
                Insight::Inbox { count } => format!("{count} inbox"),
                Insight::DoneToday { count } => format!("{count} done today"),
            });
        }
    }
    Ok(parts.join(" · "))
}

fn capture(vault: Option<&Path>, to: Option<String>, text: &str) -> Result<()> {
    let (_, root) = setup(vault)?;
    let vault =
        Vault::open_only(&root, Some(&[Kind::Project, Kind::Inbox])).map_err(|e| e.to_string())?;
    let project = match to {
        Some(name) => Some(name),
        None => vault.project_for_dir(&cwd()?).map(|p| p.name().to_string()),
    };
    let changes = vault
        .plan_capture(project.as_deref(), text)
        .map_err(|e| e.to_string())?;
    den_core::apply(&root, &changes, &vault.dirty()).map_err(|e| e.to_string())?;
    println!("captured to {}", project.as_deref().unwrap_or("inbox"));
    Ok(())
}

fn status(vault: Option<&Path>) -> Result<()> {
    let (config, root) = setup(vault)?;
    let vault = Vault::open(&root).map_err(|e| e.to_string())?;
    let log = TimerLog::load(&root, &config.machine_name()).map_err(|e| e.to_string())?;
    println!("vault    {}", root.display());
    let scope = match vault.project_for_dir(&cwd()?) {
        Some(p) => {
            println!("project  {} ({})", p.title(), p.name());
            Scope::Project(p.name().to_string())
        }
        None => {
            println!("project  none for this folder");
            Scope::All
        }
    };
    match log.running() {
        Some(r) => {
            let secs = Timestamp::now().as_second() - r.since.as_second();
            println!("timer    {} · {}", r.task, clock(secs));
        }
        None => println!("timer    not running"),
    }
    for insight in vault.insights(&scope, today()) {
        let line = match insight {
            Insight::Overdue { count, first } => format!("{count} overdue, first: {}", first.title),
            Insight::DueToday { count, first } => {
                format!("{count} due today, first: {}", first.title)
            }
            Insight::BehindPace {
                project,
                left,
                days,
            } => {
                format!("{}: {left} left, {days} days, behind pace", project.title)
            }
            Insight::Inbox { count } => format!("{count} in the inbox"),
            Insight::DoneToday { count } => format!("{count} done today"),
        };
        println!("         {line}");
    }
    let problems =
        vault.problems().len() + vault.docs().map(|d| d.parsed.issues.len()).sum::<usize>();
    if problems > 0 {
        println!("problems {problems} (see :checkhealth den)");
    }
    Ok(())
}

fn task_line(row: &TaskRow, today: Date, with_project: bool) -> String {
    let mark = match row.state {
        State::Open => "○",
        State::Doing => "◐",
        State::Dropped => "⊘",
        State::Done => "✓",
    };
    let mut line = format!("  {mark} {}", row.title);
    if with_project && let Some(p) = &row.project {
        line.push_str(&format!("  ({})", p.title));
    }
    if !row.tags.is_empty() {
        let tags: Vec<String> = row.tags.iter().map(|t| format!("#{t}")).collect();
        line.push_str(&format!("  {}", tags.join(" ")));
    }
    if let Some(due) = row.due {
        line.push_str(&format!("  {}", relative(due, today)));
    }
    line
}

fn tasks(vault: Option<&Path>, all: bool) -> Result<()> {
    let (_, root) = setup(vault)?;
    let vault = Vault::open(&root).map_err(|e| e.to_string())?;
    let scope = if all {
        Scope::All
    } else {
        match vault.project_for_dir(&cwd()?) {
            Some(p) => Scope::Project(p.name().to_string()),
            None => Scope::All,
        }
    };
    let today = today();
    let view = vault.tasks_view(&scope, today);
    if !view.doing.is_empty() {
        println!("doing");
        for row in &view.doing {
            println!("{}", task_line(row, today, scope == Scope::All));
        }
    }
    for group in &view.groups {
        println!("{}", group.label);
        for row in &group.tasks {
            println!("{}", task_line(row, today, false));
        }
    }
    if view.open == 0 {
        println!("nothing open");
    }
    Ok(())
}

fn stop(vault: Option<&Path>) -> Result<()> {
    let (config, root) = setup(vault)?;
    let mut log = TimerLog::load(&root, &config.machine_name()).map_err(|e| e.to_string())?;
    match log.stop(Timestamp::now()).map_err(|e| e.to_string())? {
        Some(r) => println!("stopped {}", r.task),
        None => println!("no timer running"),
    }
    Ok(())
}
