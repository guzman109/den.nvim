//! The engine against the fixture vault in `tests/vault`.
//!
//! Views are checked with snapshots; edits are checked byte for byte on a
//! throwaway copy of the vault.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use den_core::query::Scope;
use den_core::{Error, Section, State, TaskRef, Vault, apply};
use jiff::civil::{Date, date};
use serde::Serialize;

const TODAY: Date = date(2026, 9, 24);

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/vault")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A private copy of the fixture vault.
fn scratch() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture(), dir.path());
    let vault = Vault::open(dir.path()).unwrap();
    (dir, vault)
}

fn read(dir: &tempfile::TempDir, path: &str) -> String {
    std::fs::read_to_string(dir.path().join(path)).unwrap()
}

fn task(vault: &Vault, path: &str, text: &str) -> TaskRef {
    let doc = vault.doc(path).unwrap();
    let t = doc
        .parsed
        .tasks
        .iter()
        .find(|t| t.text.contains(text))
        .unwrap_or_else(|| panic!("no task {text:?} in {path}"));
    TaskRef {
        path: path.to_string(),
        line: t.line,
        raw: doc.buf.lines[t.line].clone(),
    }
}

fn no_dirty() -> BTreeSet<String> {
    BTreeSet::new()
}

#[derive(Serialize)]
struct DocSummary {
    path: String,
    kind: den_core::Kind,
    locked: bool,
    title: String,
    tasks: Vec<String>,
    tags: Vec<String>,
    issues: Vec<String>,
}

#[test]
fn reads_every_file_the_way_it_was_meant() {
    let vault = Vault::open(fixture()).unwrap();
    let docs: Vec<DocSummary> = vault
        .docs()
        .map(|d| DocSummary {
            path: d.path.clone(),
            kind: d.kind,
            locked: d.locked,
            title: d.title(),
            tasks: d
                .parsed
                .tasks
                .iter()
                .map(|t| format!("[{}] {}", t.state.mark(), t.title))
                .collect(),
            tags: d.parsed.tags.clone(),
            issues: d
                .parsed
                .issues
                .iter()
                .map(|i| format!("{}: {}", i.line + 1, i.message))
                .collect(),
        })
        .collect();
    insta::assert_yaml_snapshot!(docs);
    assert!(vault.problems().is_empty());
}

#[test]
fn a_sentence_that_looks_like_a_field_changes_nothing() {
    let vault = Vault::open(fixture()).unwrap();
    let edge = vault.project("edge-cases").unwrap();
    assert_eq!(edge.status(), den_core::vault::ProjectStatus::Active);
    assert_eq!(edge.title(), "Edge cases");
}

#[test]
fn tasks_across_the_vault() {
    let vault = Vault::open(fixture()).unwrap();
    insta::assert_yaml_snapshot!(vault.tasks_view(&Scope::All, TODAY));
}

#[test]
fn tasks_for_one_project_include_its_notes() {
    let vault = Vault::open(fixture()).unwrap();
    insta::assert_yaml_snapshot!(vault.tasks_view(&Scope::Project("website".into()), TODAY));
}

#[test]
fn the_inbox_gathers_every_capture() {
    let vault = Vault::open(fixture()).unwrap();
    insta::assert_yaml_snapshot!(vault.inbox_view(&Scope::All));
}

#[test]
fn insights_most_urgent_first() {
    let vault = Vault::open(fixture()).unwrap();
    insta::assert_yaml_snapshot!("insights_all", vault.insights(&Scope::All, TODAY));
    insta::assert_yaml_snapshot!(
        "insights_haste",
        vault.insights(&Scope::Project("haste".into()), TODAY)
    );
}

#[test]
fn archived_paused_and_locked_projects_stay_out_of_the_way() {
    let vault = Vault::open(fixture()).unwrap();
    let view = vault.tasks_view(&Scope::All, TODAY);
    let labels: Vec<&str> = view.groups.iter().map(|g| g.label.as_str()).collect();
    assert!(!labels.contains(&"Old blog"));
    assert!(!labels.contains(&"A quieter workspace"));
    let paused = vault.tasks_view(&Scope::Project("workspace".into()), TODAY);
    assert_eq!(
        paused.groups.len(),
        1,
        "a paused project still shows when asked for"
    );
    assert!(vault.doc("notes/private.md.age").unwrap().locked);
}

#[test]
fn finishing_a_task_changes_exactly_one_line() {
    let (dir, vault) = scratch();
    let before = read(&dir, "projects/website.md");
    let t = task(&vault, "projects/website.md", "Choose three projects");
    let changes = vault.plan_state(&t, State::Done, TODAY).unwrap();
    apply(vault.root(), &changes, &no_dirty()).unwrap();
    let expected = before.replace(
        "- [ ] Choose three projects to feature #writing\n",
        "- [x] Choose three projects to feature #writing @done(2026-09-24)\n",
    );
    assert_eq!(read(&dir, "projects/website.md"), expected);
}

#[test]
fn line_endings_survive_an_edit() {
    let (dir, vault) = scratch();
    let t = task(&vault, "projects/windows.md", "Keep my line endings");
    let changes = vault.plan_state(&t, State::Doing, TODAY).unwrap();
    apply(vault.root(), &changes, &no_dirty()).unwrap();
    assert_eq!(
        read(&dir, "projects/windows.md"),
        "---\r\nstatus: active\r\n---\r\n# Windows file\r\n\r\n## Next actions\r\n\r\n- [/] Keep my line endings\r\n"
    );
}

#[test]
fn capture_lands_at_the_end_of_the_project_inbox() {
    let (dir, vault) = scratch();
    let changes = vault
        .plan_capture(Some("haste"), "  Order more\npaper ")
        .unwrap();
    apply(vault.root(), &changes, &no_dirty()).unwrap();
    let text = read(&dir, "projects/haste.md");
    assert!(text.contains(
        "- [ ] Look up kitty's image placeholders #research\n- [ ] Order more paper\n\n## Next actions"
    ));
}

#[test]
fn capture_outside_a_project_uses_the_general_inbox() {
    let (dir, vault) = scratch();
    let changes = vault.plan_capture(None, "Buy stamps").unwrap();
    apply(vault.root(), &changes, &no_dirty()).unwrap();
    assert_eq!(
        read(&dir, "inbox.md"),
        "# Inbox\n\n- [ ] Idea: a lamp for the reference shelf\n- [ ] Buy stamps\n"
    );
}

#[test]
fn the_general_inbox_is_created_when_missing() {
    let (dir, _) = scratch();
    std::fs::remove_file(dir.path().join("inbox.md")).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    apply(
        vault.root(),
        &vault.plan_capture(None, "First").unwrap(),
        &no_dirty(),
    )
    .unwrap();
    assert_eq!(read(&dir, "inbox.md"), "# Inbox\n\n- [ ] First\n");
}

#[test]
fn promoting_moves_a_capture_into_next_actions() {
    let (dir, vault) = scratch();
    let t = task(&vault, "projects/website.md", "Find the old logo files");
    let changes = vault.plan_move(&t, None, Section::NextActions).unwrap();
    assert_eq!(changes.len(), 1);
    apply(vault.root(), &changes, &no_dirty()).unwrap();
    let text = read(&dir, "projects/website.md");
    assert!(text.contains("## Inbox\n\n## Next actions"), "{text}");
    assert!(text.contains(
        "- [x] Pick a font @done(2026-09-22)\n- [ ] Find the old logo files\n\n## Notes"
    ));
}

#[test]
fn moving_to_another_project_writes_the_target_first() {
    let (dir, vault) = scratch();
    let t = task(&vault, "projects/haste.md", "Call the printer");
    let changes = vault
        .plan_move(&t, Some("website"), Section::NextActions)
        .unwrap();
    assert_eq!(changes[0].path, "projects/website.md");
    assert_eq!(changes[1].path, "projects/haste.md");
    apply(vault.root(), &changes, &no_dirty()).unwrap();
    assert!(!read(&dir, "projects/haste.md").contains("Call the printer"));
    assert!(
        read(&dir, "projects/website.md")
            .contains("- [ ] Call the printer about the proofs\n\n## Notes")
    );
}

#[test]
fn a_nested_task_moves_with_its_children() {
    let (dir, vault) = scratch();
    let t = task(&vault, "projects/edge-cases.md", "Issue #1 is not a tag");
    let changes = vault.plan_move(&t, Some("haste"), Section::Inbox).unwrap();
    apply(vault.root(), &changes, &no_dirty()).unwrap();
    let haste = read(&dir, "projects/haste.md");
    assert!(
        haste.contains("- [ ] Issue #1 is not a tag\n  - [ ] Nested child task\n"),
        "{haste}"
    );
    assert!(!read(&dir, "projects/edge-cases.md").contains("Nested child task"));
}

#[test]
fn a_new_project_links_its_folder() {
    let (dir, vault) = scratch();
    let root = std::env::home_dir().unwrap().join("Projects/Personal/kiln");
    let changes = vault.plan_new_project("Kiln", Some(&root), TODAY).unwrap();
    apply(vault.root(), &changes, &no_dirty()).unwrap();
    assert_eq!(
        read(&dir, "projects/kiln.md"),
        "---\nroot: ~/Projects/Personal/kiln\nstatus: active\ncreated: 2026-09-24\n---\n# Kiln\n\n## Inbox\n\n## Next actions\n"
    );
    assert!(matches!(
        vault.plan_new_project("Website", None, TODAY),
        Err(Error::Exists(_))
    ));
}

#[test]
fn a_journal_page_comes_from_the_template() {
    let (dir, vault) = scratch();
    let changes = vault.plan_daily(date(2026, 9, 25)).unwrap();
    apply(vault.root(), &changes, &no_dirty()).unwrap();
    assert_eq!(
        read(&dir, "daily/2026-09-25.md"),
        "---\ndate: 2026-09-25\nmood:\n---\n# Friday 25 September\n\n## On my mind\n\n## Went well\n\n## Tomorrow\n"
    );
    assert!(
        vault.plan_daily(TODAY).unwrap().is_empty(),
        "today's page already exists"
    );
}

#[test]
fn a_note_can_join_a_project() {
    let (dir, vault) = scratch();
    let changes = vault
        .plan_new_note("Logo ideas", Some("website"), TODAY)
        .unwrap();
    apply(vault.root(), &changes, &no_dirty()).unwrap();
    assert_eq!(
        read(&dir, "notes/logo-ideas.md"),
        "---\nproject: website\ncreated: 2026-09-24\n---\n# Logo ideas\n\n"
    );
    assert!(matches!(
        vault.plan_new_note("Anything", Some("nope"), TODAY),
        Err(Error::NoProject(_))
    ));
}

#[test]
fn a_file_edited_elsewhere_is_never_overwritten() {
    let (dir, vault) = scratch();
    let t = task(&vault, "projects/website.md", "Choose three projects");
    let changes = vault.plan_state(&t, State::Done, TODAY).unwrap();
    std::fs::write(
        dir.path().join("projects/website.md"),
        "someone else's edit\n",
    )
    .unwrap();
    assert!(matches!(
        apply(vault.root(), &changes, &no_dirty()),
        Err(Error::Stale(_))
    ));
    assert_eq!(read(&dir, "projects/website.md"), "someone else's edit\n");
}

#[test]
fn unsaved_editor_text_is_read_but_never_written_over() {
    let (dir, mut vault) = scratch();
    let edited = read(&dir, "projects/website.md")
        .replace("Find the old logo files", "Find the old logo files today");
    vault
        .set_overlay("projects/website.md", Some(edited))
        .unwrap();
    let t = task(&vault, "projects/website.md", "logo files today");
    let changes = vault.plan_state(&t, State::Done, TODAY).unwrap();
    assert!(matches!(
        apply(vault.root(), &changes, &vault.dirty()),
        Err(Error::Dirty(_))
    ));
    vault.set_overlay("projects/website.md", None).unwrap();
    assert!(vault.dirty().is_empty());
    assert!(
        vault
            .doc("projects/website.md")
            .unwrap()
            .text
            .contains("Find the old logo files\n")
    );
}

#[test]
fn a_task_found_by_its_text_after_lines_moved() {
    let (dir, vault) = scratch();
    let mut t = task(&vault, "projects/website.md", "Choose three projects");
    t.line += 3;
    let changes = vault.plan_state(&t, State::Done, TODAY).unwrap();
    apply(vault.root(), &changes, &no_dirty()).unwrap();
    assert!(read(&dir, "projects/website.md").contains("- [x] Choose three projects"));
}

#[test]
fn unreadable_files_are_reported() {
    let (dir, _) = scratch();
    std::fs::write(dir.path().join("notes/binary.md"), [0xff, 0xfe, 0x00]).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    assert_eq!(vault.problems().len(), 1);
    assert_eq!(vault.problems()[0].path, "notes/binary.md");
}

#[test]
fn locked_notes_cannot_be_edited() {
    let vault = Vault::open(fixture()).unwrap();
    let r = TaskRef {
        path: "notes/private.md.age".into(),
        line: 0,
        raw: String::new(),
    };
    assert!(vault.plan_state(&r, State::Done, TODAY).is_err());
}

#[test]
fn folders_map_to_projects_through_worktrees() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let code = base.join("code/site");
    std::fs::create_dir_all(&code).unwrap();
    let git = |args: &[&str], cwd: &Path| {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"], &code);
    git(
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "i",
        ],
        &code,
    );
    git(&["worktree", "add", "-q", "../site-feature"], &code);

    let vault_dir = base.join("vault");
    std::fs::create_dir_all(vault_dir.join("projects")).unwrap();
    std::fs::write(
        vault_dir.join("projects/site.md"),
        format!("---\nroot: {}\n---\n# Site\n", code.display()),
    )
    .unwrap();
    std::fs::write(
        vault_dir.join("projects/docs.md"),
        format!("---\nroot: {}\n---\n# Docs\n", code.join("docs").display()),
    )
    .unwrap();
    std::fs::create_dir_all(code.join("docs/api")).unwrap();
    let vault = Vault::open(&vault_dir).unwrap();

    let name = |dir: &Path| vault.project_for_dir(dir).map(|p| p.name().to_string());
    assert_eq!(name(&code).as_deref(), Some("site"));
    assert_eq!(
        name(&code.join("docs/api")).as_deref(),
        Some("docs"),
        "the deepest root wins"
    );
    assert_eq!(
        name(&base.join("code/site-feature")).as_deref(),
        Some("site")
    );
    assert_eq!(name(&base), None);
}

/// Not part of the normal run: measures loading a large vault. Run with
/// `cargo test --release -p den-core --test vault -- --ignored --nocapture`.
#[test]
#[ignore = "benchmark"]
fn load_time_for_five_thousand_notes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("notes")).unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    for i in 0..100 {
        let mut text =
            format!("---\nstatus: active\n---\n# Project {i}\n\n## Inbox\n\n## Next actions\n\n");
        for t in 0..20 {
            text.push_str(&format!(
                "- [ ] Task {t} for project {i} #tag{t} @due(2026-10-{:02})\n",
                t % 28 + 1
            ));
        }
        std::fs::write(dir.path().join(format!("projects/p{i}.md")), text).unwrap();
    }
    let body = "Some prose about the thing, with a #tag and a [link](other.md).\n".repeat(40);
    for i in 0..4900 {
        std::fs::write(
            dir.path().join(format!("notes/n{i}.md")),
            format!(
                "---\nproject: p{}\n---\n# Note {i}\n\n{body}- [ ] A task\n",
                i % 100
            ),
        )
        .unwrap();
    }
    let start = std::time::Instant::now();
    let vault = Vault::open(dir.path()).unwrap();
    let loaded = start.elapsed();
    let start = std::time::Instant::now();
    let view = vault.tasks_view(&Scope::All, TODAY);
    let queried = start.elapsed();
    println!(
        "5000 files: load {loaded:?}, tasks view {queried:?} ({} open)",
        view.open
    );
}
