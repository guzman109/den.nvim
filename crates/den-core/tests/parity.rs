//! Parser parity with den.nvim.
//!
//! Both programs parse the same user-owned Markdown. If they disagree, the
//! desktop shows tasks Neovim cannot edit, or silently drops tasks the user can
//! see in the editor — so this runs against the plugin's *own* fixture table,
//! the one `den.nvim/tests/desktop.lua` asserts against.
//!
//! `tests/fixtures/tasks.json` is a vendored copy so the suite is hermetic;
//! `vendored_fixture_matches_the_plugin` catches it drifting out of date.

use std::path::PathBuf;

use den_core::vault::{Entry, Kind, Status};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    markdown: String,
    tasks: Vec<Expected>,
}

/// The shape `tests/desktop.lua` builds for comparison.
#[derive(Debug, PartialEq, Eq, Deserialize)]
struct Expected {
    caption: String,
    status: String,
    id: Option<String>,
    order: Option<u64>,
    due: Option<String>,
    invalid_metadata: bool,
}

fn vendored() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tasks.json")
}

/// The sibling den.nvim checkout, when there is one. `DEN_NVIM` overrides.
fn plugin_fixture() -> Option<PathBuf> {
    let root = std::env::var_os("DEN_NVIM")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../den.nvim"));
    let fixture = root.join("tests/fixtures/tasks.json");
    fixture.is_file().then_some(fixture)
}

#[test]
fn parses_exactly_as_den_nvim_does() {
    let raw = std::fs::read_to_string(vendored()).expect("vendored fixture");
    let cases: Vec<Case> = serde_json::from_str(&raw).expect("fixture parses");
    assert!(!cases.is_empty(), "fixture is empty");

    for case in cases {
        let entry = Entry::parse(
            PathBuf::from("/tmp/fixture.md"),
            Kind::Notes,
            &case.markdown,
            false,
        );

        // desktop.lua reports no tasks at all for an archived entry.
        let actual: Vec<Expected> = if entry.archived {
            Vec::new()
        } else {
            entry
                .tasks
                .iter()
                .map(|task| Expected {
                    caption: task.parsed.caption.clone(),
                    status: task.parsed.status.as_str().to_string(),
                    id: task.parsed.id.clone(),
                    order: task.parsed.order,
                    due: task.parsed.due.clone(),
                    invalid_metadata: task.parsed.invalid_metadata,
                })
                .collect()
        };

        assert_eq!(
            actual, case.tasks,
            "case {:?} diverges from den.nvim",
            case.name
        );
    }
}

#[test]
fn vendored_fixture_matches_the_plugin() {
    let Some(plugin) = plugin_fixture() else {
        // Nothing to compare against in this checkout; the hermetic test above
        // still ran. Set DEN_NVIM to point at the plugin.
        return;
    };
    let theirs = std::fs::read(&plugin).expect("plugin fixture");
    let ours = std::fs::read(vendored()).expect("vendored fixture");
    assert_eq!(
        ours,
        theirs,
        "tests/fixtures/tasks.json is stale; re-copy it from {}",
        plugin.display()
    );
}

/// Tags are a desktop-side reading of `@tag(name)`. den.nvim treats the token
/// as unknown metadata: it stays in the caption and survives `task.change`.
#[test]
fn tags_are_read_without_disturbing_the_caption() {
    let markdown = "# P\n- [ ] Draft the homepage story @tag(writing) @tag(deep-work) @due(2026-09-20) @id(web-story) @status(doing) @order(1000000)\n";
    let entry = Entry::parse(PathBuf::from("/tmp/p.md"), Kind::Projects, markdown, false);
    let task = entry.tasks.first().expect("one task");

    assert_eq!(task.tags(), ["writing", "deep-work"]);
    assert_eq!(task.due(), Some("2026-09-20"));
    assert_eq!(task.status(), Status::Doing);
    // den.nvim's caption keeps every token it does not own.
    assert_eq!(
        task.parsed.caption,
        "Draft the homepage story @tag(writing) @tag(deep-work) @due(2026-09-20)"
    );
    // The desktop's display strips the ones it renders as chips instead.
    assert_eq!(task.title(), "Draft the homepage story");
}

#[test]
fn fenced_and_archived_content_is_skipped() {
    let fenced = Entry::parse(
        PathBuf::from("/tmp/p.md"),
        Kind::Notes,
        "# P\n```\n- [ ] Not a task\n```\n- [ ] Real one\n",
        false,
    );
    assert_eq!(fenced.tasks.len(), 1);
    assert_eq!(fenced.tasks[0].title(), "Real one");

    let archived = Entry::parse(
        PathBuf::from("/tmp/p.md"),
        Kind::Notes,
        "# Old\nStatus: archived\n- [ ] Leave alone\n",
        false,
    );
    assert!(archived.archived);
}
