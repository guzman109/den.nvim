//! den-mcp as an AI agent sees it: the program started with a copy of the
//! fixture vault, spoken to in JSON-RPC over stdin and stdout.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

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

struct Server {
    _dir: tempfile::TempDir,
    vault: PathBuf,
    state: PathBuf,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn start() -> Server {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let vault = base.join("vault");
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/vault"),
            &vault,
        );
        let state = base.join("state");
        let mut child = Command::new(env!("CARGO_BIN_EXE_den-mcp"))
            .arg("--vault")
            .arg(&vault)
            .env("DEN_CONFIG", base.join("no-config.yaml"))
            .env("XDG_STATE_HOME", &state)
            .env("DEN_AGENT_SOCKET", base.join("agent/agent.sock"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut s = Server {
            _dir: dir,
            vault,
            state,
            child,
            stdin,
            stdout,
            next: 0,
        };
        let init = s.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }),
        );
        assert!(init["result"]["instructions"].is_string(), "{init}");
        s.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        s
    }

    fn send(&mut self, message: &Value) {
        let mut line = serde_json::to_string(message).unwrap();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let mut line = String::new();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "the server closed"
            );
            let reply: Value = serde_json::from_str(&line).unwrap();
            if reply["id"] == json!(id) {
                return reply;
            }
        }
    }

    /// Calls a tool; returns (is_error, text).
    fn call(&mut self, name: &str, arguments: Value) -> (bool, String) {
        let reply = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        let result = &reply["result"];
        assert!(result.is_object(), "{reply}");
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        (result["isError"].as_bool().unwrap_or(false), text)
    }

    fn call_ok(&mut self, name: &str, arguments: Value) -> Value {
        let (error, text) = self.call(name, arguments);
        assert!(!error, "{name}: {text}");
        serde_json::from_str(&text).unwrap()
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.vault.join(rel)).unwrap()
    }
}

#[test]
fn the_tools_are_den_actions_and_none_switches_nudges_off() {
    let mut s = Server::start();
    let list = s.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "add_task",
            "capture",
            "edit_task",
            "inbox",
            "insights",
            "journal",
            "journal_add",
            "move_task",
            "new_note",
            "nudges",
            "projects",
            "read_locked_note",
            "read_note",
            "review",
            "search",
            "set_task_state",
            "start_timer",
            "stop_timer",
            "sync_status",
            "tasks",
            "timer",
        ]
    );
    let nudges = tools.iter().find(|t| t["name"] == "nudges").unwrap();
    assert_eq!(nudges["annotations"]["readOnlyHint"], json!(true));
    assert!(
        nudges["description"]
            .as_str()
            .unwrap()
            .contains("only be turned off by the person")
    );
    let status = s.call_ok("nudges", json!({}));
    assert_eq!(status["on"], json!(true));
}

#[test]
fn reading_projects_tasks_notes_and_search() {
    let mut s = Server::start();
    let projects = s.call_ok("projects", json!({}));
    assert!(
        projects
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "website")
    );

    let tasks = s.call_ok("tasks", json!({ "project": "website" }));
    let doing = &tasks["doing"][0];
    assert_eq!(doing["title"], "Draft the homepage story");
    assert!(doing["raw"].as_str().unwrap().starts_with("- [/] Draft"));

    let note = s.call_ok("read_note", json!({ "path": "notes/homepage-story.md" }));
    assert!(note["text"].as_str().unwrap().contains("Ask two friends"));

    let hits = s.call_ok("search", json!({ "query": "LOGO" }));
    assert_eq!(hits[0]["path"], "projects/website.md");
    assert!(hits[0]["text"].as_str().unwrap().contains("logo files"));
}

#[test]
fn locked_notes_and_paths_outside_the_vault_stay_closed() {
    let mut s = Server::start();
    let (error, text) = s.call("read_note", json!({ "path": "notes/private.md.age" }));
    assert!(error && text.contains("locked"), "{text}");
    let (error, text) = s.call("read_note", json!({ "path": "../../etc/passwd" }));
    assert!(error, "{text}");
    let (error, text) = s.call("read_note", json!({ "path": "/etc/hosts" }));
    assert!(error, "{text}");
    // Without the person's unlock (no agent here), nothing opens.
    let (error, text) = s.call(
        "read_locked_note",
        json!({ "path": "notes/private.md.age" }),
    );
    assert!(error && text.contains("locked"), "{text}");
}

#[test]
fn writes_go_through_the_engine_and_stale_ones_are_refused() {
    let mut s = Server::start();
    let done = s.call_ok(
        "capture",
        json!({ "text": "Call the framer", "project": "haste" }),
    );
    assert_eq!(done["changed"][0]["path"], "projects/haste.md");
    assert!(
        s.read("projects/haste.md")
            .contains("- [ ] Call the framer")
    );

    let tasks = s.call_ok("tasks", json!({ "project": "website" }));
    let row = tasks["groups"][0]["tasks"][0].clone();
    let stale = json!({ "path": row["path"], "line": row["line"], "raw": "- [ ] Not what is there", "state": "done" });
    let (error, text) = s.call("set_task_state", stale);
    assert!(error, "{text}");
    let fine =
        json!({ "path": row["path"], "line": row["line"], "raw": row["raw"], "state": "done" });
    s.call_ok("set_task_state", fine);
    let title = row["title"].as_str().unwrap();
    assert!(
        s.read("projects/website.md")
            .contains(&format!("- [x] {title}"))
    );

    s.call_ok(
        "journal_add",
        json!({ "text": "Talked to an agent", "date": "2026-10-03" }),
    );
    assert!(s.read("daily/2026-10-03.md").contains("Talked to an agent"));
    let (error, text) = s.call("add_task", json!({ "project": "nope", "text": "x" }));
    assert!(error && text.contains("no project named nope"), "{text}");
}

#[test]
fn files_being_edited_in_neovim_are_left_alone() {
    let mut s = Server::start();
    // Neovim (this test's parent process stands in) holds unsaved changes.
    let dir = den_core::editing::dir(&s.state, &s.vault);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(std::os::unix::process::parent_id().to_string()),
        "projects/haste.md\n",
    )
    .unwrap();
    let before = s.read("projects/haste.md");
    let (error, text) = s.call(
        "capture",
        json!({ "text": "Late thought", "project": "haste" }),
    );
    assert!(error && text.contains("unsaved changes"), "{text}");
    assert_eq!(s.read("projects/haste.md"), before);
}
