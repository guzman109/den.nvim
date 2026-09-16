//! Mutation parity with den.nvim.
//!
//! `den-core::mutate::change_line` and `den.nvim/lua/den/task.lua::change` edit
//! the same files. If they disagree by a single byte, a note written through
//! the desktop and a note written through Neovim stop matching — and the
//! difference would show up as noise in the user's git history rather than as
//! an error anyone could see.
//!
//! So this runs the *actual Lua* over a table of cases and compares. It needs
//! `nvim` and a den.nvim checkout; without either it skips loudly rather than
//! passing quietly, because a silent skip on a contract test is worse than no
//! test at all.
//!
//! `DEN_NVIM` overrides the plugin location.

use std::path::PathBuf;
use std::process::Command;

use den_core::mutate::change_line;
use den_core::vault::Status;

/// `(line, status, order, id)` — chosen to cover the branches in `task.lua`:
/// each bullet form, each existing mark, metadata present and absent, unknown
/// tokens, embedded tokens, and the boundary values for `@order`.
const CASES: &[(&str, &str, Option<u64>, Option<&str>)] = &[
    ("- [ ] Plain", "doing", None, None),
    ("- [ ] Plain", "done", None, None),
    ("- [x] Finished", "backlog", None, None),
    ("* [ ] Star bullet", "doing", Some(100), None),
    ("+ [X] Plus bullet upper", "backlog", Some(0), None),
    ("  - [ ] Indented", "done", Some(9_000_000_000_000), None),
    ("- [ ] With id @id(keep)", "doing", Some(5), Some("ignored")),
    ("- [ ] No id yet", "doing", Some(5), Some("fresh-id")),
    (
        "- [ ] Existing @status(backlog) @order(1)",
        "done",
        Some(2),
        None,
    ),
    (
        "- [ ] Unknown @tag(writing) @due(2026-09-20)",
        "doing",
        Some(7),
        Some("x1"),
    ),
    (
        "  * [ ]  Keep  **my words** @custom(x) @status(backlog)",
        "doing",
        Some(200),
        Some("stable-id"),
    ),
    ("- [ ] Embedded word@status(doing)", "done", None, None),
    ("- [ ] Trailing space token @id(a) ", "doing", Some(3), None),
    (
        "- [ ] Unicode — em dash and ✓",
        "doing",
        Some(11),
        Some("u1"),
    ),
];

fn plugin_root() -> Option<PathBuf> {
    let root = std::env::var_os("DEN_NVIM")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../den.nvim"));
    root.join("lua/den/task.lua").is_file().then_some(root)
}

fn status_of(name: &str) -> Status {
    match name {
        "backlog" => Status::Backlog,
        "doing" => Status::Doing,
        "done" => Status::Done,
        other => panic!("unknown status {other}"),
    }
}

/// Runs every case through the real `task.change` in one headless Neovim.
fn lua_results(root: &PathBuf) -> Option<Vec<String>> {
    let cases: Vec<serde_json::Value> = CASES
        .iter()
        .map(|(line, status, order, id)| {
            serde_json::json!({ "line": line, "status": status, "order": order, "id": id })
        })
        .collect();
    let encoded = serde_json::to_string(&cases).expect("encode cases");

    let script = format!(
        r#"
        vim.opt.runtimepath:append({root:?})
        local task = require('den.task')
        local cases = vim.json.decode({encoded:?})
        local out = {{}}
        for _, case in ipairs(cases) do
          local order = case.order
          if order == vim.NIL then order = nil end
          local id = case.id
          if id == vim.NIL then id = nil end
          local ok, result = pcall(task.change, case.line, case.status, order, id)
          out[#out + 1] = ok and result or ('ERROR: ' .. tostring(result))
        end
        io.stdout:write(vim.json.encode(out))
        vim.cmd('qa!')
        "#,
        root = root.to_string_lossy(),
    );

    let output = Command::new("nvim")
        .args(["--headless", "--clean", "-c"])
        .arg(format!("lua {script}"))
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let start = stdout.find('[')?;
    let end = stdout.rfind(']')? + 1;
    serde_json::from_str(&stdout[start..end]).ok()
}

#[test]
fn rewrites_task_lines_byte_for_byte_like_den_nvim() {
    let Some(root) = plugin_root() else {
        eprintln!("skipped: no den.nvim checkout; set DEN_NVIM to compare against the plugin");
        return;
    };
    let Some(expected) = lua_results(&root) else {
        eprintln!("skipped: could not run nvim to produce the Lua side");
        return;
    };
    assert_eq!(
        expected.len(),
        CASES.len(),
        "nvim returned the wrong number of results"
    );

    let mut mismatches = Vec::new();
    for ((line, status, order, id), lua) in CASES.iter().zip(&expected) {
        let ours = change_line(line, status_of(status), *order, *id);

        match (&ours, lua.starts_with("ERROR:")) {
            // Both refused: the messages differ by design, agreement is enough.
            (Err(_), true) => {}
            (Ok(ours), false) if ours == lua => {}
            _ => mismatches.push(format!(
                "  {line:?} + {status}/{order:?}/{id:?}\n    lua:  {lua:?}\n    rust: {ours:?}"
            )),
        }
    }

    assert!(
        mismatches.is_empty(),
        "den-core and den.nvim disagree on {} case(s):\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

/// Lines produced by Den's *new* verbs, which have no Lua counterpart to
/// compare against directly.
///
/// `change_line` can be checked against `task.change`. `retitle_line` and the
/// line `insert_task` appends cannot — den.nvim has no equivalent. What can be
/// checked is the property that actually matters: whatever Den writes, the
/// plugin's own parser must read back identically. A line Den writes that
/// den.nvim parses differently is a silent divergence between the two
/// interfaces over the same file.
const WRITTEN: &[&str] = &[
    // As `insert_task` builds them.
    "- [ ] Captured thought @status(backlog) @order(2000000) @id(cap-1)",
    "- [ ] Minimal capture @status(backlog)",
    // As `retitle_line` leaves them.
    "- [ ] Renamed @tag(writing) @due(2026-09-20) @id(a) @status(backlog) @order(1000000)",
    "  * [x] Indented renamed @id(b) @status(done)",
    "+ [ ] No metadata at all",
    "- [ ] Unknown token kept @custom(x) @id(a) @status(backlog)",
    "- [ ] Unicode — em dash ✓ @id(u) @status(backlog)",
];

fn lua_parses(root: &PathBuf, lines: &[&str]) -> Option<Vec<serde_json::Value>> {
    let encoded = serde_json::to_string(lines).expect("encode lines");
    let script = format!(
        r#"
        vim.opt.runtimepath:append({root:?})
        local task = require('den.task')
        local lines = vim.json.decode({encoded:?})
        local out = {{}}
        for _, line in ipairs(lines) do
          local parsed = task.parse(line)
          out[#out + 1] = parsed and {{
            caption = parsed.caption,
            id = parsed.id or vim.NIL,
            status = parsed.status,
            order = parsed.order or vim.NIL,
            completed = parsed.completed,
          }} or vim.NIL
        end
        io.stdout:write(vim.json.encode(out))
        vim.cmd('qa!')
        "#,
        root = root.to_string_lossy(),
    );

    let output = Command::new("nvim")
        .args(["--headless", "--clean", "-c"])
        .arg(format!("lua {script}"))
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let start = stdout.find('[')?;
    let end = stdout.rfind(']')? + 1;
    serde_json::from_str(&stdout[start..end]).ok()
}

#[test]
fn den_nvim_reads_back_every_line_den_writes() {
    let Some(root) = plugin_root() else {
        eprintln!("skipped: no den.nvim checkout; set DEN_NVIM to compare against the plugin");
        return;
    };
    let Some(parsed) = lua_parses(&root, WRITTEN) else {
        eprintln!("skipped: could not run nvim to parse the lines");
        return;
    };
    assert_eq!(parsed.len(), WRITTEN.len());

    let mut mismatches = Vec::new();
    for (line, lua) in WRITTEN.iter().zip(&parsed) {
        let Some(ours) = den_core::vault::parse::parse_task(line) else {
            mismatches.push(format!("  {line:?}\n    rust: not a task at all"));
            continue;
        };
        if lua.is_null() {
            mismatches.push(format!("  {line:?}\n    lua: not a task, rust: parsed"));
            continue;
        }

        let lua_str = |key: &str| lua.get(key).and_then(|v| v.as_str()).map(str::to_string);
        let checks = [
            ("caption", lua_str("caption"), Some(ours.caption.clone())),
            ("id", lua_str("id"), ours.id.clone()),
            (
                "status",
                lua_str("status"),
                Some(ours.status.as_str().to_string()),
            ),
        ];
        for (field, theirs, mine) in checks {
            if theirs != mine {
                mismatches.push(format!(
                    "  {line:?}\n    {field}: lua {theirs:?} vs rust {mine:?}"
                ));
            }
        }

        let lua_order = lua.get("order").and_then(|v| v.as_u64());
        if lua_order != ours.order {
            mismatches.push(format!(
                "  {line:?}\n    order: lua {lua_order:?} vs rust {:?}",
                ours.order
            ));
        }
        let lua_completed = lua.get("completed").and_then(|v| v.as_bool());
        if lua_completed != Some(ours.completed) {
            mismatches.push(format!(
                "  {line:?}\n    completed: lua {lua_completed:?} vs rust {}",
                ours.completed
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "den.nvim parses {} line(s) Den wrote differently:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}
