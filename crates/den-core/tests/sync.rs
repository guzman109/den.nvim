//! Sync between two machines, played by two clones of one bare repository.
//!
//! Git runs with the machine's own config switched off, so signing, hooks
//! and aliases on the computer running the tests cannot change the result.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use den_core::conflict::{self, Choice};
use den_core::sync::{self, Env, Outcome, Prompt};

fn isolated() -> Vec<(String, String)> {
    [
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_AUTHOR_NAME", "Den Test"),
        ("GIT_AUTHOR_EMAIL", "den@example.invalid"),
        ("GIT_COMMITTER_NAME", "Den Test"),
        ("GIT_COMMITTER_EMAIL", "den@example.invalid"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

fn env() -> Env {
    Env {
        prompt: Prompt::Never,
        extra: isolated(),
        den: None,
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .envs(isolated())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct World {
    _dir: tempfile::TempDir,
    base: PathBuf,
    a: PathBuf,
    b: PathBuf,
}

const PROJECT: &str = "---\nstatus: active\n---\n# Website\n\nThe plan is blue.\n\n## Next actions\n\n- [ ] Register the domain\n- [ ] Write the story\n";

/// A remote holding one project, cloned by machine A and machine B.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let remote = base.join("remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-q", "--bare", "-b", "main"]);

    let a = base.join("a");
    std::fs::create_dir_all(&a).unwrap();
    sync::init(&a, &env()).unwrap();
    git(&a, &["checkout", "-q", "-b", "main"]);
    std::fs::write(a.join("projects/website.md"), PROJECT).unwrap();
    assert_eq!(sync::commit(&a, "a", &env()).unwrap(), 3);
    git(&a, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&a, &["push", "-q", "-u", "origin", "main"]);

    git(&base, &["clone", "-q", remote.to_str().unwrap(), "b"]);
    let b = base.join("b");
    World {
        _dir: dir,
        base,
        a,
        b,
    }
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

fn edit(dir: &Path, rel: &str, from: &str, to: &str) {
    let text = read(dir, rel);
    assert!(text.contains(from), "{from:?} not in {text}");
    std::fs::write(dir.join(rel), text.replacen(from, to, 1)).unwrap();
}

#[test]
fn init_lays_out_a_vault_and_leaves_existing_files_alone() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".gitignore"), "secret/").unwrap();
    std::fs::create_dir_all(dir.path().join("templates")).unwrap();
    std::fs::write(dir.path().join("templates/daily.md"), "mine").unwrap();
    sync::init(dir.path(), &env()).unwrap();
    for sub in ["projects", "notes", "daily", ".git"] {
        assert!(dir.path().join(sub).is_dir(), "{sub}");
    }
    assert_eq!(read(dir.path(), "templates/daily.md"), "mine");
    assert_eq!(
        read(dir.path(), ".gitignore"),
        "secret/\n.den/index.sqlite\n.den/*.tmp\n"
    );
    // Running it again changes nothing.
    sync::init(dir.path(), &env()).unwrap();
    assert_eq!(
        read(dir.path(), ".gitignore"),
        "secret/\n.den/index.sqlite\n.den/*.tmp\n"
    );
}

#[test]
fn an_edit_on_one_machine_reaches_the_other() {
    let w = world();
    edit(
        &w.a,
        "projects/website.md",
        "- [ ] Register",
        "- [x] Register",
    );
    assert_eq!(
        sync::run(&w.a, "a", &env()),
        Outcome::Synced {
            committed: 1,
            pulled: false,
            pushed: true,
            combined: 0
        }
    );
    assert_eq!(git(&w.a, &["log", "-1", "--format=%s"]), "den: a, 1 change");
    assert_eq!(
        sync::run(&w.b, "b", &env()),
        Outcome::Synced {
            committed: 0,
            pulled: true,
            pushed: false,
            combined: 0
        }
    );
    assert!(read(&w.b, "projects/website.md").contains("- [x] Register the domain"));
    assert_eq!(sync::run(&w.b, "b", &env()), Outcome::UpToDate);
}

#[test]
fn the_snapshot_counts_what_is_waiting() {
    let w = world();
    let clean = sync::snapshot(&w.a).unwrap();
    assert!(clean.repo);
    assert_eq!((clean.changes, clean.ahead, clean.behind), (0, 0, 0));
    assert_eq!(clean.upstream.as_deref(), Some("origin/main"));

    edit(&w.a, "projects/website.md", "blue", "green");
    std::fs::write(w.a.join("notes/new.md"), "# New\n").unwrap();
    let dirty = sync::snapshot(&w.a).unwrap();
    assert_eq!(dirty.changes, 2);
    assert_eq!(dirty.waiting(), 2);

    sync::commit(&w.a, "a", &env()).unwrap();
    let ahead = sync::snapshot(&w.a).unwrap();
    assert_eq!((ahead.changes, ahead.ahead), (0, 1));

    git(&w.a, &["push", "-q"]);
    git(&w.b, &["fetch", "-q"]);
    assert_eq!(sync::snapshot(&w.b).unwrap().behind, 1);

    let outside = tempfile::tempdir().unwrap();
    assert!(!sync::snapshot(outside.path()).unwrap().repo);
    assert_eq!(sync::run(outside.path(), "a", &env()), Outcome::NotARepo);
}

#[test]
fn task_edits_on_both_machines_combine_without_asking() {
    let w = world();
    edit(
        &w.a,
        "projects/website.md",
        "- [ ] Register the domain",
        "- [x] Register the domain @done(2026-09-24)",
    );
    assert!(matches!(
        sync::run(&w.a, "a", &env()),
        Outcome::Synced { pushed: true, .. }
    ));

    edit(
        &w.b,
        "projects/website.md",
        "- [ ] Register the domain",
        "- [ ] Register the domain #admin",
    );
    assert_eq!(
        sync::run(&w.b, "b", &env()),
        Outcome::Synced {
            committed: 1,
            pulled: true,
            pushed: true,
            combined: 1
        }
    );
    let text = read(&w.b, "projects/website.md");
    assert!(
        text.contains("- [x] Register the domain #admin @done(2026-09-24)\n"),
        "{text}"
    );
    assert!(!sync::snapshot(&w.b).unwrap().rebasing);

    sync::run(&w.a, "a", &env());
    assert_eq!(read(&w.a, "projects/website.md"), text);
}

#[test]
fn a_real_conflict_waits_for_the_person_then_finishes() {
    let w = world();
    edit(&w.a, "projects/website.md", "blue", "red");
    sync::run(&w.a, "a", &env());
    edit(&w.b, "projects/website.md", "blue", "green");

    let outcome = sync::run(&w.b, "b", &env());
    assert_eq!(
        outcome,
        Outcome::Conflict {
            files: vec!["projects/website.md".to_string()]
        }
    );
    let snap = sync::snapshot(&w.b).unwrap();
    assert!(snap.rebasing);

    // Another sync while the rebase is stopped reports the same conflict and
    // touches nothing.
    assert!(matches!(
        sync::run(&w.b, "b", &env()),
        Outcome::Conflict { .. }
    ));

    let text = read(&w.b, "projects/website.md");
    let hunks = conflict::hunks(&text);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].ours, vec!["The plan is red.".to_string()]);
    assert_eq!(hunks[0].base, Some(vec!["The plan is blue.".to_string()]));
    assert_eq!(hunks[0].theirs, vec!["The plan is green.".to_string()]);
    assert!(hunks[0].combined.is_none());

    let settled = conflict::resolve(&text, &[Choice::Theirs]).unwrap();
    std::fs::write(w.b.join("projects/website.md"), &settled).unwrap();
    assert!(matches!(
        sync::continue_after_conflict(&w.b, "b", &env()),
        Outcome::Synced { pushed: true, .. }
    ));
    assert!(!sync::snapshot(&w.b).unwrap().rebasing);

    sync::run(&w.a, "a", &env());
    assert!(read(&w.a, "projects/website.md").contains("The plan is green."));
}

#[test]
fn continuing_before_the_markers_are_gone_keeps_waiting() {
    let w = world();
    edit(&w.a, "projects/website.md", "blue", "red");
    sync::run(&w.a, "a", &env());
    edit(&w.b, "projects/website.md", "blue", "green");
    assert!(matches!(
        sync::run(&w.b, "b", &env()),
        Outcome::Conflict { .. }
    ));
    assert!(matches!(
        sync::continue_after_conflict(&w.b, "b", &env()),
        Outcome::Conflict { .. }
    ));
}

#[test]
fn a_locked_note_in_conflict_waits_for_an_explicit_choice() {
    let w = world();
    std::fs::write(w.a.join("notes/private.md.age"), "line one\nline two\n").unwrap();
    sync::run(&w.a, "a", &env());
    sync::run(&w.b, "b", &env());
    edit(&w.a, "notes/private.md.age", "line one", "from a");
    sync::run(&w.a, "a", &env());
    edit(&w.b, "notes/private.md.age", "line one", "from b");
    assert_eq!(
        sync::run(&w.b, "b", &env()),
        Outcome::Conflict {
            files: vec!["notes/private.md.age".to_string()]
        }
    );
    // Continuing without a choice must not quietly keep either side.
    std::fs::write(w.b.join("notes/private.md.age"), "from b\nline two\n").unwrap();
    assert!(matches!(
        sync::continue_after_conflict(&w.b, "b", &env()),
        Outcome::Conflict { .. }
    ));
    sync::take_side(&w.b, "notes/private.md.age", sync::Side::Mine, &env()).unwrap();
    assert!(matches!(
        sync::continue_after_conflict(&w.b, "b", &env()),
        Outcome::Synced { pushed: true, .. }
    ));
    assert_eq!(read(&w.b, "notes/private.md.age"), "from b\nline two\n");
    sync::run(&w.a, "a", &env());
    assert_eq!(read(&w.a, "notes/private.md.age"), "from b\nline two\n");
}

#[test]
fn without_a_remote_changes_are_committed_locally() {
    let dir = tempfile::tempdir().unwrap();
    sync::init(dir.path(), &env()).unwrap();
    std::fs::write(dir.path().join("inbox.md"), "- [ ] A\n").unwrap();
    assert_eq!(
        sync::run(dir.path(), "a", &env()),
        Outcome::Local { committed: 3 }
    );
    assert_eq!(sync::run(dir.path(), "a", &env()), Outcome::UpToDate);
}

#[test]
fn the_first_sync_of_a_new_vault_pushes_and_tracks_the_remote() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let remote = base.join("remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-q", "--bare", "-b", "main"]);
    let vault = base.join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    sync::init(&vault, &env()).unwrap();
    git(&vault, &["checkout", "-q", "-b", "main"]);
    git(
        &vault,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    assert_eq!(
        sync::run(&vault, "a", &env()),
        Outcome::Synced {
            committed: 2,
            pulled: false,
            pushed: true,
            combined: 0
        }
    );
    assert_eq!(
        sync::snapshot(&vault).unwrap().upstream.as_deref(),
        Some("origin/main")
    );
    assert_eq!(
        git(&remote, &["log", "-1", "--format=%s"]),
        "den: a, 2 changes"
    );
}

#[test]
fn a_second_sync_waits_its_turn() {
    let w = world();
    std::fs::write(w.a.join(".git/den-sync.lock"), "").unwrap();
    assert_eq!(sync::run(&w.a, "a", &env()), Outcome::Busy);
    std::fs::remove_file(w.a.join(".git/den-sync.lock")).unwrap();
    assert_eq!(sync::run(&w.a, "a", &env()), Outcome::UpToDate);
    assert!(!w.a.join(".git/den-sync.lock").exists());
}

/// An SSH remote whose key is locked: the fake `ssh` records how it was
/// called and answers the way real SSH does without a usable key.
#[test]
fn a_background_sync_never_prompts_for_a_locked_key() {
    let w = world();
    let record = w.base.join("ssh-called");
    let fake = w.base.join("fake-ssh");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$@ SSH_ASKPASS=$SSH_ASKPASS REQUIRE=$SSH_ASKPASS_REQUIRE\" > '{}'\necho 'git@example.invalid: Permission denied (publickey).' >&2\nexit 255\n",
            record.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(
        &w.b,
        &[
            "remote",
            "set-url",
            "origin",
            "ssh://git@example.invalid/vault.git",
        ],
    );
    git(&w.b, &["config", "core.sshCommand", fake.to_str().unwrap()]);
    edit(&w.b, "projects/website.md", "blue", "green");

    let outcome = sync::run(&w.b, "b", &env());
    assert!(
        matches!(&outcome, Outcome::KeyLocked { message } if message.contains("Permission denied")),
        "{outcome:?}"
    );
    let called = std::fs::read_to_string(&record).unwrap();
    assert!(called.contains("-o BatchMode=yes"), "{called}");
    assert!(
        called.contains("SSH_ASKPASS=false REQUIRE=force"),
        "{called}"
    );
    // The local commit is kept; only the network part waits.
    assert_eq!(sync::snapshot(&w.b).unwrap().ahead, 1);
}

#[test]
fn line_times_come_from_history() {
    let w = world();
    let first = git(&w.a, &["log", "-1", "--format=%ct"])
        .parse::<i64>()
        .unwrap();
    edit(
        &w.a,
        "projects/website.md",
        "- [ ] Write the story\n",
        "- [ ] Write the story\n- [ ] Added later\n",
    );
    let out = Command::new("git")
        .args(["commit", "-qam", "later"])
        .current_dir(&w.a)
        .envs(isolated())
        .env("GIT_COMMITTER_DATE", "2030-01-01T00:00:00Z")
        .env("GIT_AUTHOR_DATE", "2030-01-01T00:00:00Z")
        .output()
        .unwrap();
    assert!(out.status.success());

    let times = sync::line_times(&w.a, "projects/website.md").unwrap();
    assert_eq!(times["- [ ] Write the story"].as_second(), first);
    assert_eq!(
        times["- [ ] Added later"].to_string(),
        "2030-01-01T00:00:00Z"
    );
    assert!(
        sync::line_times(&w.a, "projects/missing.md")
            .unwrap()
            .is_empty()
    );
}
