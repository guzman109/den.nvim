//! The Den engine as a Lua module for Neovim: `require("den_native")`.
//!
//! One engine lives behind a lock for the whole Neovim session. Loading the
//! vault and watching it happen on background threads; they never touch Lua.
//! When one has news it queues an event and writes a byte to a pipe whose
//! read end Lua watches, and Lua calls `poll()` on its own thread to collect
//! the news.
//!
//! Every function returns plain data (tables, strings, numbers) or raises a
//! Lua error with a message meant for a person.

mod charts;

use std::collections::{BTreeMap, HashMap};
use std::io::Write as _;
use std::os::fd::IntoRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use den_core::conflict::{self, Choice};
use den_core::query::Scope;
use den_core::sync::{self, Outcome, Snapshot};
use den_core::timer::TimerLog;
use den_core::{Change, Config, Section, State, TaskRef, Vault};
use jiff::Timestamp;
use mlua::prelude::*;
use serde::Serialize;

struct Engine {
    config: Config,
    root: PathBuf,
    vault: Option<Vault>,
    log: Option<TimerLog>,
    events: Vec<Event>,
    watch: Option<den_core::watch::Watch>,
    wake: Option<std::io::PipeWriter>,
    sync: SyncState,
}

/// What the last sync did and what is waiting, for the statusline.
#[derive(Debug, Clone, Default, Serialize)]
struct SyncState {
    running: bool,
    outcome: Option<Outcome>,
    /// When the last sync finished.
    at: Option<Timestamp>,
    snapshot: Option<Snapshot>,
    /// When each committed inbox line was first written, by file and text.
    #[serde(skip)]
    ages: HashMap<String, HashMap<String, Timestamp>>,
    #[serde(skip)]
    refreshing: bool,
}

impl Engine {
    fn notify(&mut self, event: Event) {
        self.events.push(event);
        if let Some(w) = self.wake.as_mut() {
            let _ = w.write_all(b".");
        }
    }

    fn vault(&self) -> LuaResult<&Vault> {
        self.vault
            .as_ref()
            .ok_or_else(|| LuaError::runtime("Den is still loading the vault"))
    }

    fn vault_mut(&mut self) -> LuaResult<&mut Vault> {
        self.vault
            .as_mut()
            .ok_or_else(|| LuaError::runtime("Den is still loading the vault"))
    }

    fn log_mut(&mut self) -> LuaResult<&mut TimerLog> {
        self.log
            .as_mut()
            .ok_or_else(|| LuaError::runtime("Den is still loading the timer log"))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Event {
    Loaded,
    Changed {
        paths: Vec<String>,
    },
    Log,
    Sync,
    /// A background lock request finished (an unlock, the setup, a new
    /// password). `text` carries the recovery key after setup, once.
    Lock {
        op: String,
        ok: bool,
        error: Option<String>,
        text: Option<String>,
    },
    Error {
        message: String,
    },
}

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

fn lock() -> LuaResult<MutexGuard<'static, Option<Engine>>> {
    ENGINE.lock().map_err(|_| {
        LuaError::runtime("Den's engine stopped after an earlier error; restart Neovim")
    })
}

fn with<T>(f: impl FnOnce(&mut Engine) -> LuaResult<T>) -> LuaResult<T> {
    let mut guard = lock()?;
    let engine = guard
        .as_mut()
        .ok_or_else(|| LuaError::runtime("Den is not set up; call require('den').setup()"))?;
    f(engine)
}

fn err(e: den_core::Error) -> LuaError {
    LuaError::runtime(e.to_string())
}

/// Plain Lua values: `None` becomes nil rather than a null sentinel.
fn out<T: Serialize + ?Sized>(lua: &Lua, value: &T) -> LuaResult<LuaValue> {
    lua.to_value_with(
        value,
        LuaSerializeOptions::new()
            .serialize_none_to_null(false)
            .serialize_unit_to_null(false),
    )
}

fn scope(project: Option<String>) -> Scope {
    match project {
        Some(name) if !name.is_empty() => Scope::Project(name),
        _ => Scope::All,
    }
}

fn today() -> jiff::civil::Date {
    jiff::Zoned::now().date()
}

fn parse_date(value: Option<String>) -> LuaResult<jiff::civil::Date> {
    match value {
        Some(v) => v
            .parse()
            .map_err(|_| LuaError::runtime(format!("{v} is not a date"))),
        None => Ok(today()),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct SetupOptions {
    vault: Option<String>,
    machine: Option<String>,
    config: Option<String>,
}

#[derive(Serialize)]
struct SetupResult {
    fd: i32,
    root: String,
    machine: String,
    config: Config,
}

/// Loads config, opens a wake-up pipe, and starts loading the vault on a
/// background thread. Returns the pipe's read end for Lua to watch.
fn setup(lua: &Lua, opts: Option<LuaValue>) -> LuaResult<LuaValue> {
    let opts: SetupOptions = match opts {
        Some(v @ LuaValue::Table(_)) => lua.from_value(v)?,
        _ => SetupOptions::default(),
    };
    let mut config = match &opts.config {
        Some(path) => Config::load_from(Path::new(path)),
        None => Config::load(),
    }
    .map_err(err)?;
    if let Some(v) = opts.vault {
        config.vault = v;
    }
    if let Some(m) = opts.machine {
        config.machine = Some(m);
    }
    let root = config.vault_root();
    let machine = config.machine_name();
    let (reader, writer) = std::io::pipe().map_err(LuaError::external)?;
    let fd = reader.into_raw_fd();

    {
        let mut guard = lock()?;
        *guard = Some(Engine {
            config: config.clone(),
            root: root.clone(),
            vault: None,
            log: None,
            events: Vec::new(),
            watch: None,
            wake: Some(writer),
            sync: SyncState::default(),
        });
    }

    let thread_root = root.clone();
    let thread_machine = machine.clone();
    std::thread::spawn(move || load(thread_root, thread_machine));

    out(
        lua,
        &SetupResult {
            fd,
            root: root.display().to_string(),
            machine,
            config,
        },
    )
}

/// Runs on a background thread: reads the vault and log, then watches.
fn load(root: PathBuf, machine: String) {
    let vault = Vault::open(&root);
    let log = TimerLog::load(&root, &machine);
    let Ok(mut guard) = ENGINE.lock() else { return };
    let Some(engine) = guard.as_mut() else { return };
    if engine.root != root {
        return;
    }
    match (vault, log) {
        (Ok(vault), Ok(log)) => {
            engine.vault = Some(vault);
            engine.log = Some(log);
            engine.watch = den_core::watch::watch(&root, on_disk_change).ok();
            engine.notify(Event::Loaded);
        }
        (Err(e), _) | (_, Err(e)) => engine.notify(Event::Error {
            message: e.to_string(),
        }),
    }
}

/// Runs on the watcher's thread.
fn on_disk_change(changed: den_core::watch::Changed) {
    let Ok(mut guard) = ENGINE.lock() else { return };
    let Some(engine) = guard.as_mut() else { return };
    let mut paths = Vec::new();
    if let Some(vault) = engine.vault.as_mut() {
        for path in changed.docs {
            if vault.reload_if_changed(&path) {
                paths.push(path);
            }
        }
    }
    if !paths.is_empty() {
        engine.notify(Event::Changed { paths });
    }
    if changed.log
        && let Some(log) = engine.log.as_mut()
        && log.reload().is_ok()
    {
        engine.notify(Event::Log);
    }
}

fn poll(lua: &Lua, _: ()) -> LuaResult<LuaValue> {
    with(|e| out(lua, &std::mem::take(&mut e.events)))
}

fn ready(_: &Lua, _: ()) -> LuaResult<bool> {
    with(|e| Ok(e.vault.is_some() && e.log.is_some()))
}

fn tasks_view(lua: &Lua, project: Option<String>) -> LuaResult<LuaValue> {
    with(|e| out(lua, &e.vault()?.tasks_view(&scope(project), today())))
}

fn inbox_view(lua: &Lua, project: Option<String>) -> LuaResult<LuaValue> {
    with(|e| out(lua, &e.vault()?.inbox_view(&scope(project))))
}

fn insights(lua: &Lua, project: Option<String>) -> LuaResult<LuaValue> {
    with(|e| out(lua, &e.vault()?.insights(&scope(project), today())))
}

#[derive(Serialize)]
struct ProjectInfo {
    name: String,
    title: String,
    path: String,
    status: den_core::vault::ProjectStatus,
    root: Option<String>,
    due: Option<jiff::civil::Date>,
    locked: bool,
    notes: Vec<NoteInfo>,
}

#[derive(Serialize)]
struct NoteInfo {
    path: String,
    title: String,
    project: Option<String>,
    tags: Vec<String>,
    kind: den_core::Kind,
    locked: bool,
}

fn project_info(vault: &Vault, p: den_core::vault::Project<'_>) -> ProjectInfo {
    ProjectInfo {
        name: p.name().to_string(),
        title: p.title(),
        path: p.doc.path.clone(),
        status: p.status(),
        root: p.root().map(|r| r.display().to_string()),
        due: p.due(),
        locked: p.doc.locked,
        notes: vault
            .notes_of(p.name())
            .into_iter()
            .map(note_info)
            .collect(),
    }
}

fn note_info(d: &den_core::Doc) -> NoteInfo {
    NoteInfo {
        path: d.path.clone(),
        title: d.title(),
        project: d.field("project"),
        tags: d.parsed.tags.clone(),
        kind: d.kind,
        locked: d.locked,
    }
}

fn projects(lua: &Lua, _: ()) -> LuaResult<LuaValue> {
    with(|e| {
        let vault = e.vault()?;
        let list: Vec<ProjectInfo> = vault.projects().map(|p| project_info(vault, p)).collect();
        out(lua, &list)
    })
}

fn project(lua: &Lua, name: String) -> LuaResult<LuaValue> {
    with(|e| {
        let vault = e.vault()?;
        match vault.project(&name) {
            Some(p) => out(lua, &project_info(vault, p)),
            None => Ok(LuaValue::Nil),
        }
    })
}

fn project_for_dir(lua: &Lua, dir: String) -> LuaResult<LuaValue> {
    with(|e| {
        let vault = e.vault()?;
        match vault.project_for_dir(Path::new(&dir)) {
            Some(p) => out(lua, &project_info(vault, p)),
            None => Ok(LuaValue::Nil),
        }
    })
}

/// Every Markdown file Den reads, for pickers.
fn docs(lua: &Lua, _: ()) -> LuaResult<LuaValue> {
    with(|e| {
        let list: Vec<NoteInfo> = e.vault()?.docs().map(note_info).collect();
        out(lua, &list)
    })
}

/// Tasks and their state in one file, for decorating an open buffer.
fn doc_tasks(lua: &Lua, path: String) -> LuaResult<LuaValue> {
    with(|e| match e.vault()?.doc(&path) {
        Some(doc) => out(lua, &doc.parsed.tasks),
        None => Ok(LuaValue::Nil),
    })
}

fn problems(lua: &Lua, _: ()) -> LuaResult<LuaValue> {
    with(|e| {
        let vault = e.vault()?;
        let mut list: Vec<(String, String)> = vault
            .problems()
            .iter()
            .map(|p| (p.path.clone(), p.message.clone()))
            .collect();
        for doc in vault.docs() {
            for issue in &doc.parsed.issues {
                list.push((
                    format!("{}:{}", doc.path, issue.line + 1),
                    issue.message.clone(),
                ));
            }
        }
        out(lua, &list)
    })
}

fn rel(_: &Lua, abs: String) -> LuaResult<Option<String>> {
    with(|e| Ok(e.vault()?.rel(Path::new(&abs))))
}

fn abs(_: &Lua, rel: String) -> LuaResult<String> {
    with(|e| Ok(e.root.join(rel).display().to_string()))
}

fn root(_: &Lua, _: ()) -> LuaResult<String> {
    with(|e| Ok(e.root.display().to_string()))
}

fn set_overlay(_: &Lua, (path, text): (String, Option<String>)) -> LuaResult<()> {
    with(|e| {
        let vault = e.vault_mut()?;
        vault.set_overlay(&path, text).map_err(err)?;
        // Other writers (an agent's MCP server) keep off these files.
        if let Some(state) = den_core::editing::state_home() {
            den_core::editing::publish(&state, vault.root(), &vault.dirty());
        }
        Ok(())
    })
}

fn reload(_: &Lua, path: String) -> LuaResult<bool> {
    with(|e| Ok(e.vault_mut()?.reload_if_changed(&path)))
}

fn task_ref(lua: &Lua, value: LuaValue) -> LuaResult<TaskRef> {
    lua.from_value(value)
}

fn section(name: Option<String>) -> LuaResult<Section> {
    match name.as_deref() {
        None | Some("next_actions") => Ok(Section::NextActions),
        Some("inbox") => Ok(Section::Inbox),
        Some(other) => Err(LuaError::runtime(format!("unknown section {other}"))),
    }
}

fn plan_state(lua: &Lua, (task, state): (LuaValue, String)) -> LuaResult<LuaValue> {
    let task = task_ref(lua, task)?;
    let state =
        State::parse(&state).ok_or_else(|| LuaError::runtime(format!("unknown state {state}")))?;
    with(|e| {
        out(
            lua,
            &e.vault()?.plan_state(&task, state, today()).map_err(err)?,
        )
    })
}

fn plan_edit(lua: &Lua, (task, text): (LuaValue, String)) -> LuaResult<LuaValue> {
    let task = task_ref(lua, task)?;
    with(|e| out(lua, &e.vault()?.plan_edit(&task, &text).map_err(err)?))
}

fn plan_capture(lua: &Lua, (project, text): (Option<String>, String)) -> LuaResult<LuaValue> {
    with(|e| {
        out(
            lua,
            &e.vault()?
                .plan_capture(project.as_deref(), &text)
                .map_err(err)?,
        )
    })
}

fn plan_add(
    lua: &Lua,
    (project, text, sec): (String, String, Option<String>),
) -> LuaResult<LuaValue> {
    let sec = section(sec)?;
    with(|e| {
        out(
            lua,
            &e.vault()?.plan_add(&project, &text, sec).map_err(err)?,
        )
    })
}

fn plan_move(
    lua: &Lua,
    (task, project, sec): (LuaValue, Option<String>, Option<String>),
) -> LuaResult<LuaValue> {
    let task = task_ref(lua, task)?;
    let sec = section(sec)?;
    with(|e| {
        out(
            lua,
            &e.vault()?
                .plan_move(&task, project.as_deref(), sec)
                .map_err(err)?,
        )
    })
}

fn plan_new_project(lua: &Lua, (title, root): (String, Option<String>)) -> LuaResult<LuaValue> {
    with(|e| {
        let root = root.map(PathBuf::from);
        out(
            lua,
            &e.vault()?
                .plan_new_project(&title, root.as_deref(), today())
                .map_err(err)?,
        )
    })
}

fn plan_link(lua: &Lua, (project, root): (String, String)) -> LuaResult<LuaValue> {
    with(|e| {
        out(
            lua,
            &e.vault()?
                .plan_link(&project, Path::new(&root))
                .map_err(err)?,
        )
    })
}

fn plan_field(
    lua: &Lua,
    (path, key, value): (String, String, Option<String>),
) -> LuaResult<LuaValue> {
    with(|e| {
        out(
            lua,
            &e.vault()?
                .plan_field(&path, &key, value.as_deref())
                .map_err(err)?,
        )
    })
}

fn plan_new_note(lua: &Lua, (title, project): (String, Option<String>)) -> LuaResult<LuaValue> {
    with(|e| {
        out(
            lua,
            &e.vault()?
                .plan_new_note(&title, project.as_deref(), today())
                .map_err(err)?,
        )
    })
}

fn plan_daily(lua: &Lua, date: Option<String>) -> LuaResult<LuaValue> {
    let date = parse_date(date)?;
    with(|e| out(lua, &e.vault()?.plan_daily(date).map_err(err)?))
}

/// Writes changes to disk and re-reads the files they touched.
fn apply(lua: &Lua, changes: LuaValue) -> LuaResult<()> {
    let changes: Vec<Change> = lua.from_value(changes)?;
    with(|e| {
        let vault = e.vault_mut()?;
        den_core::apply(vault.root(), &changes, &vault.dirty()).map_err(err)?;
        for c in &changes {
            vault.reload(&c.path);
        }
        Ok(())
    })
}

#[derive(Serialize)]
struct RunningOut {
    file: String,
    task: String,
    since: Timestamp,
    seconds: i64,
}

fn timer_running(lua: &Lua, _: ()) -> LuaResult<LuaValue> {
    with(|e| {
        let Some(log) = e.log.as_ref() else {
            return Ok(LuaValue::Nil);
        };
        match log.running() {
            Some(r) => {
                let seconds = Timestamp::now().as_second() - r.since.as_second();
                out(
                    lua,
                    &RunningOut {
                        file: r.file,
                        task: r.task,
                        since: r.since,
                        seconds,
                    },
                )
            }
            None => Ok(LuaValue::Nil),
        }
    })
}

fn timer_start(_: &Lua, (file, task): (String, String)) -> LuaResult<()> {
    with(|e| {
        e.log_mut()?
            .start(&file, &task, Timestamp::now())
            .map_err(err)
    })
}

fn timer_stop(lua: &Lua, _: ()) -> LuaResult<LuaValue> {
    with(|e| out(lua, &e.log_mut()?.stop(Timestamp::now()).map_err(err)?))
}

fn timer_rename(
    _: &Lua,
    (from_file, from_task, file, task): (String, String, String, String),
) -> LuaResult<()> {
    with(|e| {
        e.log_mut()?
            .rename(&from_file, &from_task, &file, &task, Timestamp::now())
            .map_err(err)
    })
}

/// Seconds worked today (local calendar day), per file and in total.
fn time_today(lua: &Lua, _: ()) -> LuaResult<LuaValue> {
    with(|e| {
        let Some(log) = e.log.as_ref() else {
            return Ok(LuaValue::Nil);
        };
        let now = jiff::Zoned::now();
        let start = now.start_of_day().map_err(LuaError::external)?;
        let end = start.tomorrow().map_err(LuaError::external)?;
        let by_file: BTreeMap<String, i64> =
            log.seconds_by_file(start.timestamp(), end.timestamp(), now.timestamp());
        let total: i64 = by_file.values().sum();
        #[derive(Serialize)]
        struct Out {
            total: i64,
            by_file: BTreeMap<String, i64>,
        }
        out(lua, &Out { total, by_file })
    })
}

#[derive(Debug, Default, serde::Deserialize)]
struct SyncOptions {
    /// The program SSH and git ask secrets through; none means nothing may
    /// ask (a background sync).
    askpass: Option<String>,
    /// Neovim's server address, for the askpass to reach.
    server: Option<String>,
    /// The `den` program, for git's locked-note helpers.
    den: Option<String>,
    /// Finish a sync stopped at a conflict.
    #[serde(default)]
    resume: bool,
    /// Skip the network when nothing here is waiting to go out (after an
    /// edit settles, rather than on the regular pull).
    #[serde(default)]
    if_waiting: bool,
}

/// Starts a sync on a background thread. Returns false when one is already
/// running. An `Event::Sync` follows when it finishes.
fn sync_run(lua: &Lua, opts: Option<LuaValue>) -> LuaResult<bool> {
    let opts: SyncOptions = match opts {
        Some(v @ LuaValue::Table(_)) => lua.from_value(v)?,
        _ => SyncOptions::default(),
    };
    let started = with(|e| {
        if e.sync.running {
            return Ok(None);
        }
        e.sync.running = true;
        e.notify(Event::Sync);
        Ok(Some((e.root.clone(), e.config.machine_name())))
    })?;
    let Some((root, machine)) = started else {
        return Ok(false);
    };
    let env = sync::Env {
        den: opts.den.map(PathBuf::from),
        prompt: match opts.askpass {
            Some(program) => sync::Prompt::Askpass(PathBuf::from(program)),
            None => sync::Prompt::Never,
        },
        extra: opts
            .server
            .map(|s| vec![("DEN_NVIM".to_string(), s)])
            .unwrap_or_default(),
    };
    std::thread::spawn(move || {
        let before = sync::snapshot(&root).ok();
        let idle = opts.if_waiting
            && before
                .as_ref()
                .is_some_and(|s| s.waiting() == 0 && !s.rebasing);
        let outcome = if idle {
            None
        } else if opts.resume {
            Some(sync::continue_after_conflict(&root, &machine, &env))
        } else {
            Some(sync::run(&root, &machine, &env))
        };
        let snapshot = if idle {
            before
        } else {
            sync::snapshot(&root).ok()
        };
        let ages = capture_ages(&root);
        let Ok(mut guard) = ENGINE.lock() else { return };
        let Some(engine) = guard.as_mut() else { return };
        if engine.root != root {
            return;
        }
        engine.sync.running = false;
        if let Some(outcome) = outcome {
            engine.sync.outcome = Some(outcome);
            engine.sync.at = Some(Timestamp::now());
        }
        engine.sync.snapshot = snapshot;
        if let Some(ages) = ages {
            engine.sync.ages = ages;
        }
        engine.notify(Event::Sync);
    });
    Ok(true)
}

/// Re-reads what is waiting to sync, on a background thread.
fn sync_refresh(_: &Lua, _: ()) -> LuaResult<bool> {
    let root = with(|e| {
        if e.sync.refreshing {
            return Ok(None);
        }
        e.sync.refreshing = true;
        Ok(Some(e.root.clone()))
    })?;
    let Some(root) = root else { return Ok(false) };
    std::thread::spawn(move || {
        let snapshot = sync::snapshot(&root).ok();
        let ages = capture_ages(&root);
        let Ok(mut guard) = ENGINE.lock() else { return };
        let Some(engine) = guard.as_mut() else { return };
        if engine.root != root {
            return;
        }
        engine.sync.refreshing = false;
        engine.sync.snapshot = snapshot;
        if let Some(ages) = ages {
            engine.sync.ages = ages;
        }
        engine.notify(Event::Sync);
    });
    Ok(true)
}

/// When each inbox line was first committed, read from git history. Runs off
/// the main thread; holds the engine lock only to list the files.
fn capture_ages(root: &Path) -> Option<HashMap<String, HashMap<String, Timestamp>>> {
    let paths: Vec<String> = {
        let guard = ENGINE.lock().ok()?;
        let vault = guard.as_ref()?.vault.as_ref()?;
        let mut paths: Vec<String> = vault
            .inbox_view(&Scope::All)
            .groups
            .iter()
            .flat_map(|g| g.tasks.iter().map(|t| t.path.clone()))
            .collect();
        paths.sort();
        paths.dedup();
        paths
    };
    let mut out = HashMap::new();
    for path in paths {
        if let Ok(times) = sync::line_times(root, &path) {
            out.insert(path, times);
        }
    }
    Some(out)
}

fn sync_status(lua: &Lua, _: ()) -> LuaResult<LuaValue> {
    with(|e| out(lua, &e.sync))
}

/// Unix seconds when a line was first committed, or nil.
fn captured_at(_: &Lua, (path, line): (String, String)) -> LuaResult<Option<i64>> {
    with(|e| {
        Ok(e.sync
            .ages
            .get(&path)
            .and_then(|lines| lines.get(&line))
            .map(|t| t.as_second()))
    })
}

#[derive(Serialize)]
struct ConflictHunk {
    start: usize,
    end: usize,
    /// This machine's lines.
    mine: Vec<String>,
    /// The other machine's lines.
    other: Vec<String>,
    base: Option<Vec<String>>,
    combined: Option<Vec<String>>,
}

#[derive(Serialize)]
struct ConflictFile {
    path: String,
    /// A locked note: no lines to show, only a whole-file choice.
    locked: bool,
    /// The other machine's name, from its sync commit, when known.
    other_name: Option<String>,
    hunks: Vec<ConflictHunk>,
}

/// During `pull --rebase`, the first side of a conflict is what the other
/// machine pushed and the second is this machine's edit being replayed; in a
/// merge it is the other way round.
fn rebasing(root: &Path) -> bool {
    let git = root.join(".git");
    git.join("rebase-merge").exists() || git.join("rebase-apply").exists()
}

fn other_machine(root: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["log", "-1", "--format=%s", "HEAD"])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let subject = String::from_utf8_lossy(&out.stdout);
    let name = subject
        .trim()
        .strip_prefix("den: ")?
        .split(',')
        .next()?
        .trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The conflicts in one file, sides named for the person.
fn conflicts(lua: &Lua, path: String) -> LuaResult<LuaValue> {
    with(|e| {
        let text = std::fs::read_to_string(e.root.join(&path)).map_err(LuaError::external)?;
        let rebase = rebasing(&e.root);
        let hunks = conflict::hunks(&text)
            .into_iter()
            .map(|h| {
                let (mine, other) = if rebase {
                    (h.theirs, h.ours)
                } else {
                    (h.ours, h.theirs)
                };
                ConflictHunk {
                    start: h.start,
                    end: h.end,
                    mine,
                    other,
                    base: h.base,
                    combined: h.combined,
                }
            })
            .collect();
        let other_name = if rebase { other_machine(&e.root) } else { None };
        out(
            lua,
            &ConflictFile {
                locked: path.ends_with(".md.age"),
                path,
                other_name,
                hunks,
            },
        )
    })
}

/// Settles a conflicted file by keeping one machine's whole version:
/// `side` is "mine" or "other".
fn take_side(_: &Lua, (path, side): (String, String)) -> LuaResult<()> {
    let side = match side.as_str() {
        "mine" => sync::Side::Mine,
        "other" => sync::Side::Other,
        s => return Err(LuaError::runtime(format!("unknown side {s}"))),
    };
    let root = with(|e| Ok(e.root.clone()))?;
    sync::take_side(&root, &path, side, &sync::Env::default()).map_err(err)?;
    with(|e| {
        if let Ok(v) = e.vault_mut() {
            v.reload(&path);
        }
        Ok(())
    })
}

/// Plans settling a file's conflicts: one choice per hunk, from "combine",
/// "mine", "other", "both" and "leave".
fn plan_resolve(lua: &Lua, (path, choices): (String, Vec<String>)) -> LuaResult<LuaValue> {
    with(|e| {
        let rebase = rebasing(&e.root);
        let choices = choices
            .iter()
            .map(|c| match c.as_str() {
                "combine" => Ok(Choice::Combine),
                "mine" if rebase => Ok(Choice::Theirs),
                "mine" => Ok(Choice::Ours),
                "other" if rebase => Ok(Choice::Ours),
                "other" => Ok(Choice::Theirs),
                "both" => Ok(Choice::Both),
                "leave" => Ok(Choice::Leave),
                other => Err(LuaError::runtime(format!("unknown choice {other}"))),
            })
            .collect::<LuaResult<Vec<Choice>>>()?;
        let before = std::fs::read_to_string(e.root.join(&path)).map_err(LuaError::external)?;
        let after = conflict::resolve(&before, &choices).map_err(err)?;
        out(lua, &vec![Change::write(path, Some(before), after)])
    })
}

fn system_tz() -> jiff::tz::TimeZone {
    jiff::tz::TimeZone::system()
}

/// The Review screen's numbers for a project, or for everything.
fn review(lua: &Lua, project: Option<String>) -> LuaResult<LuaValue> {
    with(|e| {
        let vault = e.vault()?;
        let log = e
            .log
            .as_ref()
            .ok_or_else(|| LuaError::runtime("Den is still loading the timer log"))?;
        let lines = (!e.sync.ages.is_empty()).then_some(&e.sync.ages);
        let now = jiff::Zoned::now();
        out(
            lua,
            &vault.review(
                &scope(project),
                log,
                now.date(),
                now.timestamp(),
                &system_tz(),
                lines,
            ),
        )
    })
}

/// What happened on a day, for the journal page.
fn day_facts(lua: &Lua, date: Option<String>) -> LuaResult<LuaValue> {
    let date = parse_date(date)?;
    with(|e| {
        let vault = e.vault()?;
        let log = e
            .log
            .as_ref()
            .ok_or_else(|| LuaError::runtime("Den is still loading the timer log"))?;
        out(
            lua,
            &vault.day_facts(date, log, Timestamp::now(), &system_tz()),
        )
    })
}

/// Today's sunrise and sunset at the configured location.
fn sun(lua: &Lua, _: ()) -> LuaResult<LuaValue> {
    with(|e| {
        let Some(loc) = e.config.location else {
            return Ok(LuaValue::Nil);
        };
        #[derive(Serialize)]
        struct Out {
            rise: Option<Timestamp>,
            set: Option<Timestamp>,
        }
        let (rise, set) = match den_core::sun::day(today(), loc.lat, loc.lon) {
            den_core::sun::SunDay::Normal { rise, set } => (Some(rise), Some(set)),
            _ => (None, None),
        };
        out(lua, &Out { rise, set })
    })
}

#[derive(serde::Deserialize)]
struct NudgeInput {
    activity: den_core::nudge::Activity,
    #[serde(default)]
    answers: den_core::nudge::Answers,
    uid: u32,
}

fn off_state(uid: u32) -> Option<Option<jiff::civil::Date>> {
    den_core::nudge::read_seal(
        &den_core::nudge::seal_path(uid),
        0,
        Some(&den_core::nudge::back_on_path(uid)),
    )
}

/// Whether to nudge now: `nil`, or what to say.
fn nudge_check(lua: &Lua, input: LuaValue) -> LuaResult<LuaValue> {
    let input: NudgeInput = lua.from_value(input)?;
    with(|e| {
        let now = Timestamp::now();
        let today = today();
        let sunset = e
            .config
            .location
            .and_then(|loc| den_core::sun::sunset(today, loc.lat, loc.lon));
        let (start, _) = den_core::review::day_bounds(today, &system_tz());
        let walked = e
            .log
            .as_ref()
            .is_some_and(|log| log.walks(now).iter().any(|(_, end)| *end > start));
        let off = off_state(input.uid);
        let nudge = den_core::nudge::check(
            now,
            today,
            &input.activity,
            &input.answers,
            off,
            &e.config.nudges,
            sunset,
            walked,
            e.config.steps(today),
        );
        out(lua, &nudge)
    })
}

/// Whether nudges are off, and until when: `{ off = false }`, or
/// `{ off = true, until = "2026-10-01" }` (no `until`: until turned back on).
fn nudges_state(lua: &Lua, uid: u32) -> LuaResult<LuaValue> {
    #[derive(Serialize)]
    struct Out {
        off: bool,
        until: Option<jiff::civil::Date>,
        seal: String,
        /// The file to touch to turn nudges back on.
        back_on: String,
    }
    let state = off_state(uid);
    let today = today();
    let off = match state {
        Some(None) => true,
        Some(Some(until)) => until >= today,
        None => false,
    };
    out(
        lua,
        &Out {
            off,
            until: state.flatten(),
            seal: den_core::nudge::seal_path(uid).display().to_string(),
            back_on: den_core::nudge::back_on_path(uid).display().to_string(),
        },
    )
}

/// What the person sees on the way to turning nudges off, and the root
/// command that seals it once the operating system has checked it is them.
fn nudges_off_plan(lua: &Lua, (uid, until): (u32, Option<String>)) -> LuaResult<LuaValue> {
    let until = match until {
        Some(d) => Some(
            d.parse::<jiff::civil::Date>()
                .map_err(|_| LuaError::runtime(format!("{d} is not a date")))?,
        ),
        None => None,
    };
    #[derive(Serialize)]
    struct Out {
        messages: &'static [&'static str],
        prompt: &'static str,
        notice: &'static str,
        command: String,
    }
    out(
        lua,
        &Out {
            messages: den_core::nudge::OFF_MESSAGES,
            prompt: den_core::nudge::OFF_PROMPT,
            notice: den_core::nudge::AGENT_NOTICE,
            command: den_core::nudge::seal_command(uid, until),
        },
    )
}

fn walk_start(_: &Lua, _: ()) -> LuaResult<()> {
    with(|e| e.log_mut()?.walk_start(Timestamp::now()).map_err(err))
}

fn walk_end(_: &Lua, _: ()) -> LuaResult<()> {
    with(|e| e.log_mut()?.walk_end(Timestamp::now()).map_err(err))
}

/// The focus rings: this session, today against the goal, steps.
fn focus(lua: &Lua, _: ()) -> LuaResult<LuaValue> {
    with(|e| {
        #[derive(Serialize)]
        struct Out {
            session: i64,
            session_goal: i64,
            today: i64,
            today_goal: i64,
            steps: Option<u32>,
            steps_as_of: Option<Timestamp>,
            steps_goal: u32,
            task: Option<String>,
        }
        let now = jiff::Zoned::now();
        let steps = e.config.steps(now.date());
        let (start, end) = den_core::review::day_bounds(now.date(), &system_tz());
        let (session, task, today) = match e.log.as_ref() {
            Some(log) => {
                let running = log.running();
                let session = running
                    .as_ref()
                    .map_or(0, |r| now.timestamp().as_second() - r.since.as_second());
                let today: i64 = log
                    .seconds_by_file(start, end, now.timestamp())
                    .values()
                    .sum();
                (session, running.map(|r| r.task), today)
            }
            None => (0, None, 0),
        };
        out(
            lua,
            &Out {
                session,
                session_goal: i64::from(e.config.focus.session_minutes) * 60,
                today,
                today_goal: i64::from(e.config.focus.daily_goal_minutes) * 60,
                steps: steps.map(|s| s.steps),
                steps_as_of: steps.and_then(|s| s.as_of),
                steps_goal: e.config.focus.steps_goal,
                task,
            },
        )
    })
}

// Locking -------------------------------------------------------------------

/// One connection to den-agent for the whole Neovim session: in strict mode
/// the agent ties the unlocked key to it.
static AGENT: Mutex<Option<den_core::agent::Client>> = Mutex::new(None);
static AGENT_PROGRAM: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Where den-agent is (the plugin's bin/den-agent), for starting it.
fn lock_agent_program(_: &Lua, program: String) -> LuaResult<()> {
    if let Ok(mut p) = AGENT_PROGRAM.lock() {
        *p = Some(PathBuf::from(program));
    }
    Ok(())
}

fn agent_call(
    client: &mut Option<den_core::agent::Client>,
    request: &den_core::agent::Request,
) -> den_core::Result<den_core::agent::Response> {
    for attempt in 0..2 {
        if client.is_none() {
            let program = AGENT_PROGRAM
                .lock()
                .ok()
                .and_then(|p| p.clone())
                .unwrap_or_else(|| PathBuf::from("den-agent"));
            *client = Some(den_core::agent::Client::connect_or_start(&program)?);
        }
        let Some(c) = client.as_mut() else { continue };
        match c.call(request) {
            Ok(r) => return Ok(r),
            // A dropped connection (the agent restarted): reconnect once.
            Err(e) if attempt == 0 && e.to_string().starts_with("den-agent") => *client = None,
            Err(e) => return Err(e),
        }
    }
    Err(den_core::Error::Lock("den-agent is not answering".into()))
}

/// A request from Lua: `{ op = "status" }` and so on; the vault is filled in.
fn lock_request(lua: &Lua, value: LuaValue) -> LuaResult<den_core::agent::Request> {
    let mut json: serde_json::Value = lua.from_value(value)?;
    let root = with(|e| Ok(e.root.clone()))?;
    if let Some(map) = json.as_object_mut() {
        let op = map.get("op").and_then(|v| v.as_str()).unwrap_or("");
        if !matches!(op, "lock" | "stop") {
            map.insert(
                "vault".to_string(),
                serde_json::Value::String(root.display().to_string()),
            );
        }
    }
    serde_json::from_value(json).map_err(|e| LuaError::runtime(format!("lock request: {e}")))
}

#[derive(Serialize)]
struct LockOut {
    ok: bool,
    error: Option<String>,
    set_up: bool,
    unlocked: bool,
    methods: Vec<String>,
    strict: bool,
}

/// A quick request, answered now. While a slow one (a finger, a password
/// being checked) is under way, says so instead of waiting.
fn lock_call(lua: &Lua, value: LuaValue) -> LuaResult<LuaValue> {
    let request = lock_request(lua, value)?;
    let mut client = AGENT
        .try_lock()
        .map_err(|_| LuaError::runtime("Den is waiting for you to unlock"))?;
    let r = agent_call(&mut client, &request).map_err(err)?;
    out(
        lua,
        &LockOut {
            ok: r.ok,
            error: r.error.clone(),
            set_up: r.set_up,
            unlocked: r.unlocked,
            methods: r.methods.clone(),
            strict: r.strict,
        },
    )
}

/// A slow request, on a background thread; an `Event::Lock` reports it.
fn lock_call_async(lua: &Lua, value: LuaValue) -> LuaResult<()> {
    let request = lock_request(lua, value)?;
    let op = match &request {
        den_core::agent::Request::Setup { .. } => "setup",
        den_core::agent::Request::Unlock { .. } => "unlock",
        den_core::agent::Request::SetPassword { .. } => "set_password",
        den_core::agent::Request::AddYubikey { .. } => "add_yubikey",
        den_core::agent::Request::EnableTouchId { .. } => "enable_touch_id",
        _ => "other",
    }
    .to_string();
    std::thread::spawn(move || {
        let result = match AGENT.lock() {
            Ok(mut client) => agent_call(&mut client, &request),
            Err(_) => Err(den_core::Error::Lock("den-agent connection failed".into())),
        };
        drop(request);
        let event = match result {
            Ok(r) => Event::Lock {
                op,
                ok: true,
                error: None,
                text: r.text.clone(),
            },
            Err(e) => Event::Lock {
                op,
                ok: false,
                error: Some(e.to_string()),
                text: None,
            },
        };
        if let Ok(mut guard) = ENGINE.lock()
            && let Some(engine) = guard.as_mut()
        {
            engine.notify(event);
        }
    });
    Ok(())
}

fn decrypt_file(path: &str) -> LuaResult<(String, zeroize::Zeroizing<String>)> {
    let root = with(|e| Ok(e.root.clone()))?;
    if !den_core::vault::classify(path).is_some_and(|(_, locked)| locked) {
        return Err(LuaError::runtime(format!("{path} is not a locked note")));
    }
    let armored = std::fs::read_to_string(root.join(path)).map_err(LuaError::external)?;
    let mut client = AGENT
        .try_lock()
        .map_err(|_| LuaError::runtime("Den is waiting for you to unlock"))?;
    let r = agent_call(
        &mut client,
        &den_core::agent::Request::Decrypt {
            vault: root,
            text: armored.clone(),
        },
    )
    .map_err(err)?;
    Ok((
        armored,
        zeroize::Zeroizing::new(r.text.clone().unwrap_or_default()),
    ))
}

/// A locked note's text and the armored file it came from.
fn lock_read(lua: &Lua, path: String) -> LuaResult<LuaValue> {
    let (armored, text) = decrypt_file(&path)?;
    #[derive(Serialize)]
    struct Out<'a> {
        armored: String,
        text: &'a str,
    }
    out(
        lua,
        &Out {
            armored,
            text: text.as_str(),
        },
    )
}

fn plan_lock(lua: &Lua, path: String) -> LuaResult<LuaValue> {
    with(|e| out(lua, &e.vault()?.plan_lock(&path).map_err(err)?))
}

fn plan_unlock_note(lua: &Lua, path: String) -> LuaResult<LuaValue> {
    let (armored, text) = decrypt_file(&path)?;
    with(|e| {
        out(
            lua,
            &e.vault()?
                .plan_unlock(&path, &armored, &text)
                .map_err(err)?,
        )
    })
}

fn plan_write_locked(
    lua: &Lua,
    (path, before, text): (String, Option<String>, String),
) -> LuaResult<LuaValue> {
    with(|e| {
        out(
            lua,
            &e.vault()?
                .plan_write_locked(&path, before.as_deref(), &text)
                .map_err(err)?,
        )
    })
}

#[derive(serde::Deserialize)]
struct ChartData {
    #[serde(default)]
    values: Vec<f64>,
    #[serde(default)]
    days: usize,
    /// Fractions for the rings, outside in; a negative one means no data.
    #[serde(default)]
    rings: Vec<f64>,
}

/// A chart as PNG bytes: `kind` is "bars", "burndown" or "rings".
fn chart_png(lua: &Lua, (kind, data, style): (String, LuaValue, LuaValue)) -> LuaResult<LuaString> {
    let data: ChartData = lua.from_value(data)?;
    let style: charts::Style = lua.from_value(style)?;
    let svg = match kind.as_str() {
        "bars" => charts::bars(&data.values, &style),
        "burndown" => charts::burndown(&data.values, data.days, &style),
        "rings" => {
            let r = |i: usize| data.rings.get(i).copied().filter(|f| *f >= 0.0);
            charts::rings([r(0), r(1), r(2)], &style)
        }
        other => return Err(LuaError::runtime(format!("unknown chart {other}"))),
    };
    let bytes = charts::png(&svg, style.width, style.height).map_err(LuaError::runtime)?;
    lua.create_string(&bytes)
}

fn config(lua: &Lua, _: ()) -> LuaResult<LuaValue> {
    with(|e| out(lua, &e.config))
}

fn today_str(_: &Lua, _: ()) -> LuaResult<String> {
    Ok(today().to_string())
}

fn version(_: &Lua, _: ()) -> LuaResult<String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[allow(unsafe_code)]
#[mlua::lua_module]
fn den_native(lua: &Lua) -> LuaResult<LuaTable> {
    let m = lua.create_table()?;
    m.set("version", lua.create_function(version)?)?;
    m.set("setup", lua.create_function(setup)?)?;
    m.set("poll", lua.create_function(poll)?)?;
    m.set("ready", lua.create_function(ready)?)?;
    m.set("today", lua.create_function(today_str)?)?;
    m.set("config", lua.create_function(config)?)?;
    m.set("root", lua.create_function(root)?)?;
    m.set("rel", lua.create_function(rel)?)?;
    m.set("abs", lua.create_function(abs)?)?;
    m.set("tasks_view", lua.create_function(tasks_view)?)?;
    m.set("inbox_view", lua.create_function(inbox_view)?)?;
    m.set("insights", lua.create_function(insights)?)?;
    m.set("projects", lua.create_function(projects)?)?;
    m.set("project", lua.create_function(project)?)?;
    m.set("project_for_dir", lua.create_function(project_for_dir)?)?;
    m.set("docs", lua.create_function(docs)?)?;
    m.set("doc_tasks", lua.create_function(doc_tasks)?)?;
    m.set("problems", lua.create_function(problems)?)?;
    m.set("set_overlay", lua.create_function(set_overlay)?)?;
    m.set("reload", lua.create_function(reload)?)?;
    m.set("plan_state", lua.create_function(plan_state)?)?;
    m.set("plan_edit", lua.create_function(plan_edit)?)?;
    m.set("plan_capture", lua.create_function(plan_capture)?)?;
    m.set("plan_add", lua.create_function(plan_add)?)?;
    m.set("plan_move", lua.create_function(plan_move)?)?;
    m.set("plan_new_project", lua.create_function(plan_new_project)?)?;
    m.set("plan_link", lua.create_function(plan_link)?)?;
    m.set("plan_field", lua.create_function(plan_field)?)?;
    m.set("plan_new_note", lua.create_function(plan_new_note)?)?;
    m.set("plan_daily", lua.create_function(plan_daily)?)?;
    m.set("apply", lua.create_function(apply)?)?;
    m.set("timer_running", lua.create_function(timer_running)?)?;
    m.set("timer_start", lua.create_function(timer_start)?)?;
    m.set("timer_stop", lua.create_function(timer_stop)?)?;
    m.set("timer_rename", lua.create_function(timer_rename)?)?;
    m.set("time_today", lua.create_function(time_today)?)?;
    m.set("sync_run", lua.create_function(sync_run)?)?;
    m.set("sync_refresh", lua.create_function(sync_refresh)?)?;
    m.set("sync_status", lua.create_function(sync_status)?)?;
    m.set("captured_at", lua.create_function(captured_at)?)?;
    m.set("conflicts", lua.create_function(conflicts)?)?;
    m.set("plan_resolve", lua.create_function(plan_resolve)?)?;
    m.set("take_side", lua.create_function(take_side)?)?;
    m.set("review", lua.create_function(review)?)?;
    m.set("day_facts", lua.create_function(day_facts)?)?;
    m.set("sun", lua.create_function(sun)?)?;
    m.set("nudge_check", lua.create_function(nudge_check)?)?;
    m.set("nudges_state", lua.create_function(nudges_state)?)?;
    m.set("nudges_off_plan", lua.create_function(nudges_off_plan)?)?;
    m.set("walk_start", lua.create_function(walk_start)?)?;
    m.set("walk_end", lua.create_function(walk_end)?)?;
    m.set("focus", lua.create_function(focus)?)?;
    m.set("chart_png", lua.create_function(chart_png)?)?;
    m.set(
        "lock_agent_program",
        lua.create_function(lock_agent_program)?,
    )?;
    m.set("lock_call", lua.create_function(lock_call)?)?;
    m.set("lock_call_async", lua.create_function(lock_call_async)?)?;
    m.set("lock_read", lua.create_function(lock_read)?)?;
    m.set("plan_lock", lua.create_function(plan_lock)?)?;
    m.set("plan_unlock_note", lua.create_function(plan_unlock_note)?)?;
    m.set("plan_write_locked", lua.create_function(plan_write_locked)?)?;
    Ok(m)
}
