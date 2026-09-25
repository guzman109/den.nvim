# Logs

A dated record of work sessions and decisions, newest first. Each entry says
what happened, what was decided, what was reversed and why, and what comes
next. A change to [PLAN.md](PLAN.md) always gets an entry here.

```markdown
## YYYY-MM-DD · Title
- Did:
- Decided:
- Reversed:
- Next:
```

---

## 2026-09-24 · Build: M4, timer, statusline, `den`

**Did**
- The statusline component: the running timer, then at most two insights
  (engine insights and registered extras compete by priority), then status
  items such as sync. It caches what it shows and never calls the engine
  while drawing; a one-second tick runs only while a timer does.
- The `den` command: `prompt`, `capture`, `status`, `tasks`, `stop`.
  `den prompt` reads only the project files and this machine's timer log
  (new engine entry points `Vault::open_only`, `TimerLog::load_own`):
  median 3.3 ms over 50 runs, release build.
- README with install, keys, statusline (mini.statusline and plain) and
  starship setup.

**Decided**
- `den prompt` never prints an error; `den status` is where problems show.
- The new Den keeps the `den` Lua module name because it replaces den.nvim;
  the README says to remove the old plugin.
- New dependency: `clap`, for the `den` command.

**Found and fixed:** B-003 (the Neovim test summary could be lost, and a
partial run could pass).

**Next:** M5, sync.

---

## 2026-09-24 · Build: M3, the Neovim plugin

**Did**
- den-nvim: the engine as `require("den_native")`. One engine per session
  behind a lock; the vault loads and is watched on background threads that
  wake Lua through a pipe and never touch Lua themselves.
- Lua: setup, buffer-aware applying of plans, a shared screen renderer, the
  Tasks and Inbox screens, the capture box, the one-time question about
  unknown code folders, decorations (due dates, linked notes), pickers
  (fzf-lua, else vim.ui.select), highlights, `:Den` commands,
  `:checkhealth den`, the journal page.
- 21 headless Neovim tests (`scripts/test-nvim.sh`), each test file on a
  fresh copy of the fixture vault.
- Rendered the screens with the owner's real config (ember + markview) and
  looked at them.

**Decided**
- A change to a file open with unsaved edits goes into the buffer and stays
  unsaved; Den never saves someone's edits for them. A file open without
  edits is changed and written at once.
- Filtering is `f` and help is `g?`, so `/` and `?` stay Neovim's search.
- Columns size to their contents; titles in project groups use the empty
  project column.
- Project names on screens are quiet (`Comment`), not `Directory`, which is
  bold coral in ember.
- Den's decorations sit above markview's highlights (priority 5000).
- mlua's generated entry point needs `unsafe`; den-nvim relaxes the lint to
  `deny` and allows it on that one function.

**Found and fixed**
- Trimming trailing spaces left highlight ranges past the end of a line.
- The Tasks view scanned every file per task (see the M0–M2 entry).

**Next:** M4 — timer in the statusline, the `den` command, starship.

---

## 2026-09-24 · Build: M0–M2, the engine

**Did**
- Removed the old app (crates, demo, mockup, packaging, bundled fonts); kept
  `assets/icons`.
- New workspace: `den-core`, `den-nvim`, `den-cli`; toolchain pinned to
  1.98.1; workspace lints; GitLab CI (Linux only — shared runners have no
  macOS, so macOS runs locally).
- Fixture vault in `tests/vault` covering every format rule and the edge
  cases (fences, bad dates, prose before the title, a field-looking sentence,
  CRLF, nested tasks, unknown marks, a locked note).
- den-core: parser, frontmatter, vault (overlays, worktree-aware project
  lookup, problems report), queries (Tasks, Inbox, insights, pace), edit
  planning (state, edit, capture, add, move with children, new project, link,
  new note, journal page), atomic writer, timer log, config, file watcher.
  72 tests, snapshots reviewed by hand.

**Numbers:** a synthetic 5,000-file vault loads in ~118 ms (release build).
The Tasks view first took 225 ms because every project lookup scanned every
file; looking projects up by path and grouping notes once brought it to
6.6 ms.

**Decided**
- A capture outside any project goes to `inbox.md` at the vault root.
- Plans carry each file's full text before and after, so Neovim can apply a
  change to an open buffer instead of the disk.
- Removing a task's only line from a section also removes the blank line it
  leaves doubled.
- New dependency: `gethostname`, for the default machine name in the timer log.

**Found**
- The owner's global git config signs every commit with an SSH key. Tests now
  run git with no global config. For M5: Den's background commits will be
  signed too, so a locked key must pause sync instead of hanging it.

**Next:** M3, the Neovim plugin.

---

## 2026-09-24 · Design session: Den from scratch

**Did**
- Reviewed the old app and its engine. The design had grown screens without a
  clear job ("too much"), and the engine was a parser plus one write, shaped by
  the old desktop screens. Findings are in [BUGS.md](BUGS.md) as lessons L1–L9.
- Decided to start from complete scratch: no compatibility with the old vault
  format or code.
- Drew every Neovim screen on the design canvas
  (https://claude.ai/artifact/ADbMiGmm9Mve26G3d2ZPYF): project file (raw and
  with markview), Tasks, timer, Review (text and kitty charts), statusline
  insights, journal, break nudge and its off switch, Inbox, capture, sync
  passphrase and conflict, unlock, new-folder prompt, linked notes, focus rings.
- Wrote PLAN, TASKS, BUGS, LOGS and CONVENTIONS.

**Decided**
- Den is a full client for a Markdown vault. Neovim first; the desktop app
  gets the same features with its own UI later; the agent comes last.
- One Rust engine, synchronous, returning data. Neovim loads it as a Lua module.
- Format: YAML frontmatter, title from the first heading, line order,
  `[ ] [/] [-] [x]`, `#tag`, `@due(...)`, `@done(...)`. Matches what markview
  already draws.
- Projects are folders on disk linked by `root:`; one central vault with
  `projects/`, `notes/`, `daily/`, `templates/`.
- Git is the backend: one repo for all notes, automatic commit / pull / push,
  `git` command for writes, gitoxide for reads, askpass inside Neovim, SHA-1
  for now.
- Per-machine JSONL log only for timer sessions.
- Per-note locking with a vault key, den-agent, Touch ID / Linux password /
  YubiKey / recovery key. Designed now, built in M7.
- Daily journal with a short template; facts shown, not written.
- Break nudges with a human-only off switch.
- Search: fzf-lua + skim + ripgrep in Neovim; nucleo later for the desktop.

**Reversed**
- *UI parity → feature parity.* The first sketches drew the desktop app as a
  copy of the Neovim screens. It should have the same features with a richer
  UI of its own; Neovim goes first.
- *Tasks in Markdown → tasks in a database → back to Markdown.* A database
  (then per-machine event logs rebuilt into SQLite) was considered so views
  could be pure queries. Rejected on review: merges would depend on machine
  clocks, git would show JSON instead of tasks, editing tasks as text would
  need diff translation, logs grow forever. The query layer is the engine's
  in-memory model built from the files; the log keeps only timer sessions.
- *Header fields "anywhere" → frontmatter.* Once there were no existing files
  to stay compatible with, frontmatter was the clearer choice.

**Considered and rejected:** neuxdb, KDE Vault / encrypted disk images,
passkeys, SHA-256 repositories today, git2, skim as a crate, tokio in the
engine. Reasons are in PLAN.md → *Decided against*.

**Next:** M0. Create the `rebuild` branch, remove the old crates, set up the
new workspace, CI and the fixture vault.
