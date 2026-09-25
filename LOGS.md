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

## 2026-09-24 · Build: M6, review, journal, nudges

**Did**
- den-core `review`: finished per day (14 days), time per day this week and
  by project, progress per project, burndowns for projects with an end date
  (open tasks count from their first commit when history is known), and a
  day's facts for the journal.
- den-core `sun`: sunrise and sunset offline, within seconds of NOAA's
  calculator; polar days have none.
- den-core `nudge`: when to nudge (chair time, sunset without a walk),
  answers (snooze, not today), rotating messages, and the "off" seal that
  only counts when root owns it and its folder.
- The Review screen (`:Den review`), focus rings (`:Den focus`, a corner
  window redrawn every second), day facts under a journal page's title, the
  late-day journal line, the nudge window and `:Den break`,
  `:Den nudges off|on`.
- Images in kitty: den-nvim draws charts as shape-only SVG with resvg (no
  fonts) and Lua places them with kitty's Unicode placeholders, so they are
  ordinary buffer text to Neovim. Block-character charts everywhere else.
  Looked at the PNGs; the text screens were checked by eye.
- Tests: 4 review, 2 sun, 7 nudge (including seal ownership and
  permissions), 1 config, 2 chart renders, 10 Neovim.

**Decided**
- No `sunrise` crate: it brings `chrono` next to `jiff` for 40 lines of
  arithmetic. No `base64` crate: Neovim has `vim.base64`.
- Den's own kitty code rather than snacks.nvim (spike closed): about 150
  lines, no dependency, and it fits Den's screens.
- "Nudges off" is a root-owned seal written behind the operating system's
  password dialog (macOS `osascript … with administrator privileges`, Linux
  `pkexec`). It only runs fixed commands as root, never Den's own binary,
  which the person can overwrite.
- The journal line is the quietest insight: it shows only when fewer urgent
  things compete for the two places.

**Not yet:** steps (HealthKit spike), a real look in kitty, and a
fingerprint signature on the seal (M7).

**Next:** prebuilt downloads (M3 leftover), then M7, locking.

---

## 2026-09-24 · Build: M5, sync

**Did**
- den-core `sync`: gitoxide reads (changes waiting, ahead/behind, upstream,
  a stopped rebase, conflicted files, when each line was first committed);
  git-command writes (commit, `pull --rebase --autostash` with diff3
  conflicts, push, first push with tracking). A lock file keeps two syncs
  apart. Three prompt modes: never (background), askpass (Neovim), terminal
  (`den sync` in a shell).
- den-core `conflict`: reads diff3 hunks, combines task edits that do not
  overlap, settles a hunk by choice. Sync combines automatically when every
  hunk in every file combines, and otherwise stops for the person.
- `den sync [--continue]`, `den init [folder] [--remote url]`, and `den` as
  an askpass program: SSH runs it with the prompt, it asks Neovim over
  Neovim's msgpack-RPC socket and prints the answer.
- Neovim: background syncs (start, quiet period after edits, every few
  minutes), `:Den sync [continue]`, the Conflicts screen, a quiet statusline
  segment that only speaks when something is wrong, capture ages on the Inbox
  (`3d`), sync lines in `:checkhealth den`.
- Tests: 11 engine sync tests against a bare remote and two clones
  (including a fake `ssh` that proves background mode passes
  `BatchMode=yes` and never asks), 10 conflict unit tests, 2 CLI tests,
  8 Neovim tests (askpass end to end through the real binary, a conflict
  settled on the screen, a locked key).
- Checked against the owner's real git setup: background commits are
  signed through the SSH agent (87 ms) without prompting.

**Decided**
- Conflicts that are only task edits to different fields combine without
  asking (PLAN.md, Sync). Everything else still stops.
- Syncs wait while a vault buffer has unsaved edits.
- On quit, a detached `den sync` sends what is waiting.
- Askpass is the `den` binary itself (`DEN_ASKPASS=1`), not a subcommand,
  because SSH runs the askpass program with only the prompt as argument.
- `den init --remote` exists because identity and signing keys are often
  chosen by remote (`includeIf hasconfig:remote.*.url`); without a remote a
  new repository may have no identity at all, and git says so.
- New dependency: `rmpv` (planned), for talking to Neovim.

**Next:** M6, review, journal and nudges.

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
