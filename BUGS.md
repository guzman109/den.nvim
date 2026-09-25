# Bugs

Every bug gets an entry here before it is fixed, and every fix gets a
regression test. Closed bugs stay, under **Closed**, so the history is easy to
search.

## Template

```markdown
### B-000 · Short title
- Found: YYYY-MM-DD, by whom or what
- Status: open | fixing | closed (commit)
- Where: crate / file / screen
- Steps: how to make it happen
- Expected: what should happen
- Actual: what happens
- Fix: what changed and why
- Test: the regression test that now guards it
```

## Open

None.

## Closed

### B-004 · Neovim crashed during the tests, after `:checkhealth`
- Found: 2026-09-24, the Neovim test run ending early (about one run in
  three with M7's code, one in twelve before it)
- Status: closed (worked around in the tests)
- Where: tests/nvim/test_commands.lua; the crash is inside Neovim 0.12.5
- Steps: run `:checkhealth den`, then wipe every buffer (`:%bwipeout!` or
  `nvim_buf_delete`), repeatedly in one session
- Expected: the buffers go
- Actual: Neovim dies in `aubuflocal_remove` (a use-after-free while
  removing buffer-local autocommands), sometimes later as a malloc abort in
  whatever allocates next. Den creates no buffer-local autocommands at that
  moment; the report buffer's own are involved.
- Fix: the test runs Den's checks directly and collects what they report,
  so no report buffer is made. Worth reporting upstream with a minimal
  reproduction.
- Test: 15 runs of the file in a row pass

### B-005 · The tests could start a real den-agent
- Found: 2026-09-24, a `den-agent` left running after a test run
- Status: closed
- Where: tests/nvim/run.lua
- Steps: a test opens the fixture's locked note, which asks the agent
- Expected: tests never touch the person's own agent or socket
- Actual: the agent was started on the default socket and outlived the run
- Fix: the whole run uses a private socket and stops its agent at the end
- Test: `pgrep den-agent` finds nothing after `scripts/test-nvim.sh`

### B-006 · A conflicted locked note would have been settled silently
- Found: 2026-09-24, while designing locked-note merges (before it shipped)
- Status: closed
- Where: den-core sync.rs `continue_after_conflict`
- Steps: two machines change the same lines of a locked note; sync stops;
  run `den sync --continue`
- Expected: the person chooses whose version to keep
- Actual: an encrypted file has no conflict markers, so it looked settled
  and one machine's edit would have been dropped
- Fix: locked notes are skipped by the automatic settling and need an
  explicit choice (`take_side`, `m`/`t` on the Conflicts screen)
- Test: `a_locked_note_in_conflict_waits_for_an_explicit_choice`

### B-001 · The Tasks view took 225 ms on a large vault
- Found: 2026-09-24, measuring a synthetic 5,000-file vault
- Status: closed (42132e8)
- Where: den-core query.rs, vault.rs
- Steps: `cargo test --release -p den-core --test vault -- --ignored --nocapture`
- Expected: a few milliseconds
- Actual: 225 ms; every project lookup scanned every file, once per task
- Fix: projects are looked up by path; each view groups notes once
- Test: `load_time_for_five_thousand_notes` (6.6 ms after)

### B-002 · Highlights past the end of a screen line
- Found: 2026-09-24, Neovim tests
- Status: closed (3509438)
- Where: lua/den/ui/screen.lua
- Steps: open the Tasks screen with a row that has trailing spaces
- Expected: the screen draws
- Actual: "Invalid 'col': out of range" — trimming spaces left highlight ranges
  beyond the line
- Fix: highlight ranges are clamped to the line's length
- Test: every screen test draws rows with trimmed ends

### B-003 · Neovim test summary lost when output went to a file
- Found: 2026-09-24, running `scripts/test-nvim.sh > log`
- Status: closed
- Where: tests/nvim/run.lua, scripts/test-nvim.sh
- Steps: redirect the test run to a file
- Expected: every result line and the summary
- Actual: the last buffered lines, including the summary, were lost at exit,
  so a run looked like it stopped early — and would have passed even if it had
- Fix: output is line-buffered and flushed before exit; the script fails when
  the summary line is missing
- Test: the script's own check

## Lessons from the old engine

Found while reviewing the previous `den-core` (commit `eaa701d`) on
2026-09-24. That code is not being kept; these are here so the rebuild does
not repeat them.

| # | What went wrong | What the rebuild does instead |
|---|---|---|
| L1 | The index created and queried a backlinks table that nothing ever wrote to, so backlinks were always empty | Every query gets a test with real data behind it |
| L2 | Contract tests against the Lua plugin looked for it at the wrong path and printed "skipped" to hidden output, so they passed without running | Tests fail when something they need is missing; no silent skips |
| L3 | The atomic write created the new file with default permissions (a `600` note became `644`), replaced symlinked notes with plain files, and did not fsync the folder | One writer that keeps permissions, writes through symlinks and fsyncs file and folder |
| L4 | `Status:` and `Stack:` lines were read anywhere in a file, including inside code blocks; a sentence in the notes could archive a project | Fields only in frontmatter at the top; nothing inside code fences is parsed |
| L5 | Status was stored twice (checkbox and `@status`). Ticking a "doing" task in another editor and unticking it later brought it back as "doing" | The checkbox is the only state: `[ ] [/] [-] [x]` |
| L6 | The history ledger lived outside the vault and stored task text, leaking what the index move into `.den/` was meant to protect | Timer lines for locked notes store no text; nothing sensitive outside the vault |
| L7 | Screen-specific helpers (date formats for one screen, meter fractions, pinned tag colours for a demo) lived in the engine | The engine returns data; formatting belongs to each interface |
| L8 | Unreadable and non-UTF-8 files were skipped without a word | They are reported |
| L9 | Only one write existed (rewrite one task line), which forced machine tokens (`@order`, `@id`) into notes | The engine can insert, move and delete lines safely |
