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
