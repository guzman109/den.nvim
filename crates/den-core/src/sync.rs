//! Keeping the vault in step with its git remote.
//!
//! Reads (what changed, how far ahead or behind, where a line came from) use
//! gitoxide, in process. Writes (commit, pull, push) run the `git` command, so
//! the person's SSH config, keys, agent, credential helpers and encryption
//! filters all apply exactly as they do in a terminal.
//!
//! A background sync must never ask for anything: a passphrase or host-key
//! question would land on the editor's terminal or hang forever. In
//! background mode every way git or SSH could prompt is shut, and a sync
//! that needs a secret reports [`Outcome::KeyLocked`] instead. A foreground
//! sync points SSH at an askpass helper, which asks inside the editor.
//! (gpg-agent's own pinentry, for GPG-signed commits, is outside Den's
//! reach and may still appear.)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use jiff::Timestamp;
use serde::Serialize;

use crate::error::{Error, Result};

/// How git may ask the person for a secret.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Prompt {
    /// Background sync: nothing may ask. A needed secret stops the sync
    /// with [`Outcome::KeyLocked`].
    #[default]
    Never,
    /// SSH and git ask through this program (Den's askpass, which asks
    /// inside the editor).
    Askpass(PathBuf),
    /// Run from a shell: SSH and git ask on the terminal as usual.
    Terminal,
}

#[derive(Debug, Clone, Default)]
pub struct Env {
    pub prompt: Prompt,
    /// The `den` program, for git's locked-note diff and merge helpers.
    /// When set and the vault uses locking, sync makes sure this clone's
    /// git knows about them.
    pub den: Option<PathBuf>,
    /// Extra environment for every git run (the editor's address for the
    /// askpass; tests use it to isolate git from the machine's own config).
    pub extra: Vec<(String, String)>,
}

/// What a sync did, or why it stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// Nothing to commit, pull or push.
    UpToDate,
    Synced {
        committed: usize,
        pulled: bool,
        pushed: bool,
        /// Conflicts settled by combining both machines' task edits.
        combined: usize,
    },
    /// Committed locally; the repository has no upstream to pull or push.
    Local {
        committed: usize,
    },
    /// A secret (SSH passphrase, signing key) is needed, and this sync may
    /// not ask for it.
    KeyLocked {
        message: String,
    },
    /// The remote could not be reached.
    Offline {
        message: String,
    },
    /// Both sides changed the same lines. The rebase is stopped with markers
    /// in these files.
    Conflict {
        files: Vec<String>,
    },
    /// Another sync holds the lock.
    Busy,
    NotARepo,
    Failed {
        message: String,
    },
}

/// The repository's state, read without running git.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub repo: bool,
    /// Files changed, added or removed since the last commit.
    pub changes: usize,
    /// Commits here that the upstream does not have.
    pub ahead: usize,
    /// Commits on the upstream (as last fetched) that are not here.
    pub behind: usize,
    pub upstream: Option<String>,
    /// A rebase stopped part way (after a conflict).
    pub rebasing: bool,
    /// Files with unresolved conflicts.
    pub conflicts: Vec<String>,
}

impl Snapshot {
    /// Commits and changes not yet on the remote.
    pub fn waiting(&self) -> usize {
        self.ahead + self.changes
    }
}

fn gix_err(e: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("git: {e}"))
}

/// Reads the repository state with gitoxide.
pub fn snapshot(root: &Path) -> Result<Snapshot> {
    let repo = match gix::open(root) {
        Ok(repo) => repo,
        Err(_) => return Ok(Snapshot::default()),
    };
    let mut snap = Snapshot {
        repo: true,
        ..Snapshot::default()
    };

    let git_dir = repo.git_dir();
    snap.rebasing = git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists();
    if let Ok(index) = repo.index_or_empty() {
        let mut conflicts: Vec<String> = index
            .entries()
            .iter()
            .filter(|e| e.stage_raw() != 0)
            .map(|e| e.path(&index).to_string())
            .collect();
        conflicts.sort();
        conflicts.dedup();
        snap.conflicts = conflicts;
    }

    let status = repo
        .status(gix::progress::Discard)
        .map_err(gix_err)?
        .untracked_files(gix::status::UntrackedFiles::Files)
        .into_iter(Vec::<gix::bstr::BString>::new())
        .map_err(gix_err)?;
    // A file both staged and changed again shows up twice; count it once.
    let mut paths: Vec<gix::bstr::BString> = status
        .filter_map(|item| item.ok())
        .map(|item| item.location().to_owned())
        .collect();
    paths.sort();
    paths.dedup();
    snap.changes = paths.len();

    let Ok(head) = repo.head_id() else {
        return Ok(snap);
    };
    let head = head.detach();
    let upstream = repo.head_name().ok().flatten().and_then(|name| {
        repo.branch_remote_tracking_ref_name(name.as_ref(), gix::remote::Direction::Fetch)?
            .ok()
    });
    if let Some(upstream_name) = upstream {
        snap.upstream = Some(upstream_name.shorten().to_string());
        if let Ok(mut reference) = repo.find_reference(upstream_name.as_ref())
            && let Ok(up) = reference.peel_to_id()
        {
            let up = up.detach();
            let count = |from: gix::ObjectId, hide: gix::ObjectId| -> usize {
                repo.rev_walk([from])
                    .with_hidden([hide])
                    .all()
                    .map(|walk| walk.filter_map(|c| c.ok()).count())
                    .unwrap_or(0)
            };
            snap.ahead = count(head, up);
            snap.behind = count(up, head);
        }
    }
    Ok(snap)
}

/// When each committed line of a file first appeared, by its text. Lines not
/// committed yet are absent. The earliest time wins for repeated lines.
pub fn line_times(root: &Path, path: &str) -> Result<HashMap<String, Timestamp>> {
    let repo = gix::open(root).map_err(gix_err)?;
    let head = repo.head_id().map_err(gix_err)?.detach();
    let outcome = match repo.blame_file(path.into(), head, Default::default()) {
        Ok(outcome) => outcome,
        Err(_) => return Ok(HashMap::new()),
    };
    let text = String::from_utf8_lossy(&outcome.blob).into_owned();
    let lines: Vec<&str> = text.lines().collect();
    let mut times: HashMap<gix::ObjectId, Option<Timestamp>> = HashMap::new();
    let mut out: HashMap<String, Timestamp> = HashMap::new();
    for entry in &outcome.entries {
        let time = *times.entry(entry.commit_id).or_insert_with(|| {
            let commit = repo.find_commit(entry.commit_id).ok()?;
            let seconds = commit.time().ok()?.seconds;
            Timestamp::from_second(seconds).ok()
        });
        let Some(time) = time else { continue };
        let start = entry.start_in_blamed_file as usize;
        for line in lines.iter().skip(start).take(entry.len.get() as usize) {
            let slot = out.entry((*line).to_string()).or_insert(time);
            if time < *slot {
                *slot = time;
            }
        }
    }
    Ok(out)
}

/// Runs git in the vault with the environment for this kind of sync.
fn git(root: &Path, env: &Env, args: &[&str]) -> Result<Output> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(root)
        .env("GIT_EDITOR", "true")
        .env("GIT_MERGE_AUTOEDIT", "no");
    match &env.prompt {
        Prompt::Terminal => {}
        Prompt::Askpass(program) => {
            cmd.stdin(Stdio::null())
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("SSH_ASKPASS", program)
                .env("SSH_ASKPASS_REQUIRE", "force")
                .env("GIT_ASKPASS", program)
                .env("DEN_ASKPASS", "1");
        }
        Prompt::Never => {
            cmd.stdin(Stdio::null()).env("GIT_TERMINAL_PROMPT", "0");
            // Any passphrase question goes to a program that always refuses,
            // so it fails at once instead of reaching a terminal.
            cmd.env("SSH_ASKPASS", "false")
                .env("SSH_ASKPASS_REQUIRE", "force")
                .env("GIT_ASKPASS", "false");
            let ssh = ssh_command(root, env);
            cmd.env("GIT_SSH_COMMAND", format!("{ssh} -o BatchMode=yes"));
        }
    }
    for (k, v) in &env.extra {
        cmd.env(k, v);
    }
    cmd.output().map_err(|e| Error::io(root, e))
}

/// The SSH command the person configured, so background mode only adds to it.
fn ssh_command(root: &Path, env: &Env) -> String {
    if let Ok(cmd) = std::env::var("GIT_SSH_COMMAND")
        && !cmd.trim().is_empty()
    {
        return cmd;
    }
    let mut probe = Command::new("git");
    probe
        .args(["config", "--get", "core.sshCommand"])
        .current_dir(root)
        .stdin(Stdio::null());
    for (k, v) in &env.extra {
        probe.env(k, v);
    }
    match probe.output() {
        Ok(out) if out.status.success() => {
            let cmd = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if cmd.is_empty() {
                "ssh".to_string()
            } else {
                cmd
            }
        }
        _ => "ssh".to_string(),
    }
}

fn stderr(out: &Output) -> String {
    let mut text = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if text.is_empty() {
        text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    }
    text
}

/// Sorts a failed git run into what the person needs to know.
fn classify_failure(message: String) -> Outcome {
    let lower = message.to_lowercase();
    let key = [
        "permission denied (publickey",
        "passphrase",
        "couldn't load",
        "load key",
        "signing failed",
        "failed to sign",
        "ssh_askpass",
        "host key verification failed",
    ];
    let offline = [
        "could not resolve host",
        "network is unreachable",
        "connection refused",
        "connection timed out",
        "operation timed out",
        "no route to host",
    ];
    if key.iter().any(|k| lower.contains(k)) {
        Outcome::KeyLocked { message }
    } else if offline.iter().any(|k| lower.contains(k)) {
        Outcome::Offline { message }
    } else {
        Outcome::Failed { message }
    }
}

/// The repository's own folder: `.git`, or where a `.git` file points (a
/// linked worktree, a submodule, a separate git dir).
pub fn git_dir(root: &Path) -> Option<PathBuf> {
    gix::open(root)
        .ok()
        .map(|repo| repo.git_dir().to_path_buf())
}

/// Whether a rebase stopped part way.
pub fn rebasing(root: &Path) -> bool {
    git_dir(root)
        .is_some_and(|g| g.join("rebase-merge").exists() || g.join("rebase-apply").exists())
}

/// The commit a stopped rebase is replaying onto: what the other machine
/// pushed.
pub fn rebase_onto(root: &Path) -> Option<String> {
    let g = git_dir(root)?;
    ["rebase-merge/onto", "rebase-apply/onto"]
        .iter()
        .find_map(|p| std::fs::read_to_string(g.join(p)).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Holds the sync lock (in the repository's own folder) for the length of a
/// sync. The file names the
/// process holding it; a lock whose process is gone was left by a crash and
/// is broken. A slow sync (waiting for a passphrase) keeps its lock.
struct Lock(PathBuf);

fn alive(pid: i32) -> bool {
    // Signal 0 checks the process exists without touching it.
    !matches!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
        Err(nix::errno::Errno::ESRCH)
    )
}

impl Lock {
    fn take(root: &Path) -> Option<Lock> {
        use std::io::Write as _;
        let path = git_dir(root)?.join("den-sync.lock");
        if let Ok(text) = std::fs::read_to_string(&path) {
            let holder = text.trim().parse::<i32>().ok();
            let stale = match holder {
                Some(pid) => !alive(pid),
                // An old lock without a process id: stale after a while.
                None => std::fs::metadata(&path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|m| m.elapsed().ok())
                    .is_some_and(|age| age.as_secs() > 3600),
            };
            if stale {
                let _ = std::fs::remove_file(&path);
            }
        }
        let mut file = std::fs::File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .ok()?;
        let _ = write!(file, "{}", std::process::id());
        Some(Lock(path))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Commits every change in the vault. Returns how many files it included.
pub fn commit(root: &Path, machine: &str, env: &Env) -> std::result::Result<usize, Outcome> {
    let run = |args: &[&str]| {
        git(root, env, args).map_err(|e| Outcome::Failed {
            message: e.to_string(),
        })
    };
    let add = run(&["add", "-A"])?;
    if !add.status.success() {
        return Err(Outcome::Failed {
            message: stderr(&add),
        });
    }
    let staged = run(&["diff", "--cached", "--name-only", "-z"])?;
    let files = staged
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .count();
    if files == 0 {
        return Ok(0);
    }
    let noun = if files == 1 { "change" } else { "changes" };
    let message = format!("den: {machine}, {files} {noun}");
    let out = run(&["commit", "-q", "-m", &message])?;
    if !out.status.success() {
        // Leave the changes staged-but-uncommitted exactly as they were.
        let _ = run(&["reset", "-q"]);
        return Err(classify_failure(stderr(&out)));
    }
    Ok(files)
}

/// Commits, pulls (rebasing local work on top), and pushes.
pub fn run(root: &Path, machine: &str, env: &Env) -> Outcome {
    if !root.join(".git").exists() {
        return Outcome::NotARepo;
    }
    let Some(_lock) = Lock::take(root) else {
        return Outcome::Busy;
    };
    if let Some(den) = &env.den
        && crate::lock::is_set_up(root)
        && let Err(e) = crate::lock::git_setup(root, den)
    {
        return Outcome::Failed {
            message: e.to_string(),
        };
    }
    if let Ok(snap) = snapshot(root)
        && (snap.rebasing || !snap.conflicts.is_empty())
    {
        return Outcome::Conflict {
            files: snap.conflicts,
        };
    }
    let committed = match commit(root, machine, env) {
        Ok(n) => n,
        Err(outcome) => return outcome,
    };
    let has_upstream = git(
        root,
        env,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .map(|o| o.status.success())
    .unwrap_or(false);
    if !has_upstream {
        return first_push(root, env, committed);
    }

    let before = head(root, env);
    // No autostash: an edit saved while the fetch runs would come back from
    // the stash with conflicts git still calls a success, and with the
    // sides the other way round. Instead, a pull refused for a dirty tree
    // commits that edit and tries once more.
    let pull_once = || {
        git(
            root,
            env,
            &[
                "-c",
                "merge.conflictStyle=diff3",
                "pull",
                "--rebase",
                "--no-autostash",
                "-q",
            ],
        )
    };
    let mut committed = committed;
    let mut pull = match pull_once() {
        Ok(out) => out,
        Err(e) => {
            return Outcome::Failed {
                message: e.to_string(),
            };
        }
    };
    let dirty_tree = |out: &Output| {
        let text = stderr(out).to_lowercase();
        text.contains("unstaged changes") || text.contains("uncommitted changes")
    };
    if !pull.status.success() && dirty_tree(&pull) {
        match commit(root, machine, env) {
            Ok(n) => committed += n,
            Err(outcome) => return outcome,
        }
        pull = match pull_once() {
            Ok(out) => out,
            Err(e) => {
                return Outcome::Failed {
                    message: e.to_string(),
                };
            }
        };
    }
    let mut combined = 0;
    if !pull.status.success() {
        let stopped = snapshot(root).is_ok_and(|s| s.rebasing || !s.conflicts.is_empty());
        if !stopped {
            return classify_failure(stderr(&pull));
        }
        match settle(root, env) {
            Ok(n) => combined = n,
            Err(outcome) => return outcome,
        }
    }
    // Whatever git said, a file still in conflict is a conflict.
    if let Ok(s) = snapshot(root)
        && (s.rebasing || !s.conflicts.is_empty())
    {
        return Outcome::Conflict { files: s.conflicts };
    }
    let pulled = head(root, env) != before;

    let ahead = snapshot(root).map(|s| s.ahead).unwrap_or(0);
    let mut pushed = false;
    if ahead > 0 {
        match git(root, env, &["push", "-q"]) {
            Ok(out) if out.status.success() => pushed = true,
            Ok(out) => return classify_failure(stderr(&out)),
            Err(e) => {
                return Outcome::Failed {
                    message: e.to_string(),
                };
            }
        }
    }
    if committed == 0 && !pulled && !pushed {
        Outcome::UpToDate
    } else {
        Outcome::Synced {
            committed,
            pulled,
            pushed,
            combined,
        }
    }
}

/// Carries a stopped rebase through when every conflict is task edits that
/// combine. Returns how many conflicts were combined, or the conflict the
/// person has to settle (the rebase stays stopped, markers in place).
fn settle(root: &Path, env: &Env) -> std::result::Result<usize, Outcome> {
    let mut combined = 0;
    // One round per replayed commit; a vault syncs often, so a handful is
    // plenty and the bound only guards against a loop.
    for _ in 0..50 {
        let snap = snapshot(root).map_err(|e| Outcome::Failed {
            message: e.to_string(),
        })?;
        if !snap.rebasing {
            return Ok(combined);
        }
        let mut settled = Vec::new();
        for file in &snap.conflicts {
            let path = root.join(file);
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            match crate::conflict::combine_all(&text) {
                Some(result) => {
                    settled.push((file, path, crate::conflict::hunks(&text).len(), result))
                }
                None => {
                    return Err(Outcome::Conflict {
                        files: snap.conflicts.clone(),
                    });
                }
            }
        }
        for (file, path, count, result) in settled {
            let path = crate::write::resolve(root, file)
                .map(|_| path)
                .map_err(|e| Outcome::Failed {
                    message: e.to_string(),
                })?;
            crate::write::write_atomically(&path, result.as_bytes()).map_err(|e| {
                Outcome::Failed {
                    message: e.to_string(),
                }
            })?;
            let added = git(root, env, &["add", "--", file]).map_err(|e| Outcome::Failed {
                message: e.to_string(),
            })?;
            if !added.status.success() {
                return Err(Outcome::Failed {
                    message: stderr(&added),
                });
            }
            combined += count;
        }
        let out = git(root, env, &["rebase", "--continue"]).map_err(|e| Outcome::Failed {
            message: e.to_string(),
        })?;
        if !out.status.success() {
            let stopped = snapshot(root).is_ok_and(|s| !s.conflicts.is_empty());
            if !stopped {
                return Err(classify_failure(stderr(&out)));
            }
        }
    }
    let files = snapshot(root).map(|s| s.conflicts).unwrap_or_default();
    Err(Outcome::Conflict { files })
}

/// A branch with no upstream yet: pushed to the first remote and tracked
/// from then on. With no remote at all the commit stays local.
fn first_push(root: &Path, env: &Env, committed: usize) -> Outcome {
    let remotes = git(root, env, &["remote"])
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let Some(remote) = remotes.lines().map(str::trim).find(|r| !r.is_empty()) else {
        return if committed > 0 {
            Outcome::Local { committed }
        } else {
            Outcome::UpToDate
        };
    };
    if head(root, env).is_empty() {
        return Outcome::UpToDate;
    }
    match git(root, env, &["push", "-q", "-u", remote, "HEAD"]) {
        Ok(out) if out.status.success() => Outcome::Synced {
            committed,
            pulled: false,
            pushed: true,
            combined: 0,
        },
        Ok(out) => classify_failure(stderr(&out)),
        Err(e) => Outcome::Failed {
            message: e.to_string(),
        },
    }
}

fn head(root: &Path, env: &Env) -> String {
    git(root, env, &["rev-parse", "HEAD"])
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// After every conflicted file has been resolved and written: marks them
/// resolved, finishes the rebase, and pushes.
pub fn continue_after_conflict(root: &Path, machine: &str, env: &Env) -> Outcome {
    let snap = match snapshot(root) {
        Ok(s) => s,
        Err(e) => {
            return Outcome::Failed {
                message: e.to_string(),
            };
        }
    };
    for file in &snap.conflicts {
        // A locked note shows no markers (it is encrypted), so only an
        // explicit choice (`take_side`) settles it.
        if file.ends_with(".md.age") {
            continue;
        }
        let text = std::fs::read_to_string(root.join(file)).unwrap_or_default();
        if crate::conflict::hunks(&text).is_empty() {
            let _ = git(root, env, &["add", "--", file]);
        }
    }
    let snap = match snapshot(root) {
        Ok(s) => s,
        Err(e) => {
            return Outcome::Failed {
                message: e.to_string(),
            };
        }
    };
    if !snap.conflicts.is_empty() {
        return Outcome::Conflict {
            files: snap.conflicts,
        };
    }
    if snap.rebasing {
        match git(root, env, &["rebase", "--continue"]) {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                if let Ok(s) = snapshot(root)
                    && !s.conflicts.is_empty()
                {
                    return Outcome::Conflict { files: s.conflicts };
                }
                return classify_failure(stderr(&out));
            }
            Err(e) => {
                return Outcome::Failed {
                    message: e.to_string(),
                };
            }
        }
    }
    run(root, machine, env)
}

/// Which version of a conflicted file to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// This machine's.
    Mine,
    /// The other machine's.
    Other,
}

/// Settles a conflicted file by keeping one machine's whole version, and
/// marks it settled. For locked notes, whose conflicts cannot be shown line
/// by line.
pub fn take_side(root: &Path, path: &str, side: Side, env: &Env) -> Result<()> {
    // Den only ever stops in a rebase: stage 2 is what the other machine
    // pushed and stage 3 this machine's replayed edit.
    let stage = match side {
        Side::Mine => 3,
        Side::Other => 2,
    };
    let out = git(root, env, &["show", &format!(":{stage}:{path}")])?;
    if !out.status.success() {
        return Err(Error::Invalid(format!("{path}: {}", stderr(&out))));
    }
    let target = crate::write::resolve(root, path)?;
    crate::write::write_atomically(&target, &out.stdout)?;
    let added = git(root, env, &["add", "--", path])?;
    if !added.status.success() {
        return Err(Error::Invalid(format!(
            "git add {path}: {}",
            stderr(&added)
        )));
    }
    Ok(())
}

/// Makes `root` a vault repository: the folders, the journal template, a
/// `.gitignore` for derived files, and a first commit. Existing files are
/// left alone.
pub fn init(root: &Path, env: &Env) -> Result<()> {
    for dir in ["projects", "notes", "daily", "templates"] {
        std::fs::create_dir_all(root.join(dir)).map_err(|e| Error::io(root.join(dir), e))?;
    }
    let template = root.join("templates/daily.md");
    if !template.exists() {
        std::fs::write(&template, crate::ops::DEFAULT_DAILY_TEMPLATE)
            .map_err(|e| Error::io(&template, e))?;
    }
    let ignore = root.join(".gitignore");
    let wanted = ".den/index.sqlite\n.den/*.tmp\n.*.den-*.tmp\n";
    let current = std::fs::read_to_string(&ignore).unwrap_or_default();
    if !current.contains(".den/index.sqlite") {
        let mut text = current;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(wanted);
        std::fs::write(&ignore, text).map_err(|e| Error::io(&ignore, e))?;
    }
    if !root.join(".git").exists() {
        let out = git(root, env, &["init", "-q"])?;
        if !out.status.success() {
            return Err(Error::Invalid(format!("git init: {}", stderr(&out))));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_are_sorted_into_what_to_tell_the_person() {
        assert!(matches!(
            classify_failure("git@host: Permission denied (publickey).".into()),
            Outcome::KeyLocked { .. }
        ));
        assert!(matches!(
            classify_failure("error: Load key \"/x\": incorrect passphrase supplied".into()),
            Outcome::KeyLocked { .. }
        ));
        assert!(matches!(
            classify_failure("ssh: Could not resolve host gitlab.com".into()),
            Outcome::Offline { .. }
        ));
        assert!(matches!(
            classify_failure("fatal: something else".into()),
            Outcome::Failed { .. }
        ));
    }
}
