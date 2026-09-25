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

use std::collections::BTreeMap;
use std::io::Write as _;
use std::os::fd::IntoRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use den_core::query::Scope;
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
    Changed { paths: Vec<String> },
    Log,
    Error { message: String },
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
    with(|e| e.vault_mut()?.set_overlay(&path, text).map_err(err))
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
    Ok(m)
}
