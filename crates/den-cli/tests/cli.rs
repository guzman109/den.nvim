//! The `den` command, run as a program against a copy of the fixture vault.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

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

struct Setup {
    _dir: tempfile::TempDir,
    vault: PathBuf,
    code: PathBuf,
}

/// A vault copy whose `haste` project points at a code folder in the temp dir.
fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let vault = base.join("vault");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/vault"),
        &vault,
    );
    let code = base.join("code/haste");
    std::fs::create_dir_all(&code).unwrap();
    let haste = vault.join("projects/haste.md");
    let text = std::fs::read_to_string(&haste).unwrap().replace(
        "root: ~/Projects/Personal/haste",
        &format!("root: {}", code.display()),
    );
    std::fs::write(&haste, text).unwrap();
    Setup {
        _dir: dir,
        vault,
        code,
    }
}

fn den(s: &Setup, cwd: &Path, args: &[&str]) -> (String, String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_den"))
        .args(args)
        .arg("--vault")
        .arg(&s.vault)
        .current_dir(cwd)
        .env("DEN_CONFIG", s.vault.join("no-config.yaml"))
        .output()
        .unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
        out.status.success(),
    )
}

#[test]
fn the_prompt_names_the_most_urgent_thing_in_this_folder() {
    let s = setup();
    let (out, err, ok) = den(&s, &s.code, &["prompt"]);
    assert!(ok, "{err}");
    // haste's "Profile the first frame" is due 2026-09-24 and its inbox holds
    // two captures; which one is most urgent depends on today's date.
    let today = jiff::Zoned::now().date();
    let due = jiff::civil::date(2026, 9, 24);
    let expected = if today == due {
        "due today\n"
    } else if today > due {
        "1 overdue\n"
    } else {
        "2 inbox\n"
    };
    assert_eq!(out, expected);
}

#[test]
fn the_prompt_is_empty_outside_any_project() {
    let s = setup();
    let elsewhere = s.vault.parent().unwrap().to_path_buf();
    let (out, _, ok) = den(&s, &elsewhere, &["prompt"]);
    assert!(ok);
    assert_eq!(out, "");
}

#[test]
fn the_prompt_never_fails_even_without_a_vault() {
    let s = setup();
    let out = Command::new(env!("CARGO_BIN_EXE_den"))
        .args(["prompt", "--vault", "/nonexistent/vault"])
        .env("DEN_CONFIG", s.vault.join("no-config.yaml"))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
}

#[test]
fn capture_from_the_shell_lands_in_the_folder_project() {
    let s = setup();
    let (out, err, ok) = den(&s, &s.code, &["capture", "Call", "the", "framer"]);
    assert!(ok, "{err}");
    assert_eq!(out, "captured to haste\n");
    let text = std::fs::read_to_string(s.vault.join("projects/haste.md")).unwrap();
    assert!(
        text.contains("- [ ] Call the framer\n\n## Next actions"),
        "{text}"
    );
}

#[test]
fn capture_can_name_another_project() {
    let s = setup();
    let (out, _, ok) = den(&s, &s.code, &["capture", "--to", "website", "Buy a domain"]);
    assert!(ok);
    assert_eq!(out, "captured to website\n");
    let text = std::fs::read_to_string(s.vault.join("projects/website.md")).unwrap();
    assert!(text.contains("- [ ] Buy a domain"));
}

#[test]
fn an_unknown_project_is_an_error_not_a_guess() {
    let s = setup();
    let (_, err, ok) = den(&s, &s.code, &["capture", "--to", "nope", "x"]);
    assert!(!ok);
    assert!(err.contains("no project named nope"), "{err}");
}

#[test]
fn status_and_tasks_describe_this_folder() {
    let s = setup();
    let (out, err, ok) = den(&s, &s.code, &["status"]);
    assert!(ok, "{err}");
    assert!(out.contains("project  haste (haste)"), "{out}");
    let (out, _, ok) = den(&s, &s.code, &["tasks"]);
    assert!(ok);
    assert!(out.contains("◐ Wire up the renderer"), "{out}");
    assert!(out.contains("○ Profile the first frame  #rust"), "{out}");
    assert!(!out.contains("Personal website"), "{out}");
}

#[test]
fn the_timer_stops_from_the_shell() {
    let s = setup();
    let (out, _, ok) = den(&s, &s.code, &["stop"]);
    assert!(ok);
    assert_eq!(
        out, "no timer running\n",
        "the fixture log belongs to another machine"
    );
}
