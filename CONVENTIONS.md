# Conventions

Rules for anyone — person or agent — writing code or docs for Den. When a rule
gets in the way, change the rule here first, with a line in
[LOGS.md](LOGS.md), rather than breaking it quietly.

## Principles

1. **Markdown is the truth.** If it isn't in the vault's Markdown, it is a log
   (timer sessions) or a cache (index) and can be rebuilt or lost without
   losing notes.
2. **The engine returns data.** Never pre-formatted text for a grid, never
   colours, never screen-specific date strings. Interfaces format.
3. **Synchronous core.** Async code only at the edges (desktop, MCP), calling
   the engine from a worker thread.
4. **One write path.** Every change to a vault file goes through the engine's
   atomic writer.
5. **Never lose a byte.** When unsure whether a write is safe, refuse it and
   say why.
6. **Quiet by default.** A new screen element needs a reason to exist.

## Rust

- Edition 2024; toolchain pinned in `rust-toolchain.toml`.
- `cargo fmt` and `cargo clippy -- -D warnings` pass before every commit.
- Libraries define errors with `thiserror`. No `unwrap()` or `expect()`
  outside tests, except for a true invariant, with a message saying why it
  holds.
- Nothing may panic across the mlua boundary: every function exported to Lua
  returns a `Result` and turns errors into Lua errors.
- No `unsafe`, except in den-agent's memory locking, where each block
  explains what it relies on.
- Keep dependencies to the list in [PLAN.md](PLAN.md). Adding one needs a line
  in LOGS.md saying why.

## Files and the vault

- Writes go through one function: temp file in the same folder, fsync the
  file, rename, fsync the folder, keep the original permissions, write through
  symlinks to the real file.
- Before writing, check the file still matches what was read. If not, refuse
  and reload.
- Refuse to write a file that has unsaved changes in a Neovim buffer.
- Never write outside the vault. Never touch a code repository.
- Report files Den cannot read (permissions, not UTF-8). Don't skip silently.

## Threads

- Nothing slow on Neovim's main thread (see PLAN.md → *Threading*).
- Never touch Lua from another thread. Background work stores its result and
  wakes Neovim through a pipe.
- `git` runs as a separate process; Neovim watches it without waiting.

## Tests

- Tests use the fixture vault in the repository or a `tempfile` copy of it.
  Never a real vault.
- Parser output and screen text use `insta` snapshots.
- Operations are tested byte for byte: exact input file, exact output file.
- Every bug in [BUGS.md](BUGS.md) gets a regression test named after it.
- A test that needs a tool (git, Neovim) fails when the tool is missing. No
  silent skips.

## Comments and docs

- Comments say what the code does now and why. No history ("this used to…"),
  no references to screens or mockups.
- Public items get a doc comment. Private ones only when the why isn't
  obvious.

## Neovim

- Everything is under `:Den <subcommand>` and `require("den")`.
- Den's own screens (`den://…`) may use single-letter keys, but never shadow
  built-in keys people use in lists: `n`, `N`, `gn`, `/`, `?` stay Neovim's
  (`/` may filter only where noted in PLAN.md).
- Den adds no default key mappings to note buffers. It only adds virtual text
  (due dates, linked notes, the day's facts).
- Highlight groups are named `Den*` and linked to standard groups, so any
  colour scheme works; ember is the one designs are drawn in.
- Never store secrets in Neovim: passphrases go straight to SSH, locked notes
  use buffers with swap and undo files off.

## Words on screen

- Sentence case, short, plain. No exclamation marks.
- Say what happened, then what to do: `sync paused · key locked`.
- Playful guilt is allowed in break nudges and nowhere else.

## Security

- Never log or store passphrases, keys or decrypted text.
- Locked notes never reach the index, the timer log, swap or undo files, or a
  temp file on disk.
- Anything an agent can call goes through the engine's checks. Turning nudges
  off and unlocking notes need a human.

## Git

- Subject in the imperative, under ~60 characters; the body explains why.
- Small commits; one change per commit.
- Work on a branch; main stays green.

## Keeping these files current

- **TASKS.md:** mark tasks `[/]` when starting and `[x] … @done(date)` when
  finished; new work goes under *Inbox* until sorted.
- **LOGS.md:** one entry per work session or decision.
- **BUGS.md:** a bug is written down before it is fixed.
- **PLAN.md:** changes only with a LOGS.md entry explaining why.
