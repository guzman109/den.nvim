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

### B-011 · Locked notes never worked on Linux
- Found: 2026-09-25, the first CI run on Linux (GitHub Actions)
- Status: fixing (the fix waits for a Linux CI run to confirm it)
- Where: crates/den-agent/src/platform.rs, crates/den-core/src/agent.rs
- Steps: on Linux, unlock the vault, or run `cargo test -p den-agent`
- Expected: Neovim, `den` and `den-mcp` talk to den-agent
- Actual: every client refuses the agent ("could not tell which program is
  listening"); all 11 agent tests time out with "the agent starts"
- Cause: the agent made itself non-dumpable, which hides
  `/proc/<pid>/exe` from other processes, and that is exactly what
  clients read to check the listener is den-agent. Only macOS had run the
  tests before.
- Fix: the agent stays dumpable on Linux. The program check protects the
  password; memory reads are left to the kernel's ptrace rules. Crash
  dumps stay off
- Test: tests/agent.rs, on the Linux CI runner

### B-010 · Tests assume today is 2026-09-24
- Found: 2026-09-25, the Neovim suite on the day after the fixtures' date
- Status: open
- Where: tests/nvim/test_statusline.lua, test_sync.lua; the fixture vault
- Steps: run `scripts/test-nvim.sh` on any day after 2026-09-24
- Expected: the tests pass on any day
- Actual: 4 fail. "Profile the first frame" (due 2026-09-24) turns
  overdue, and the fixture's timer session left running on 2026-09-24
  shows in the statusline
- Fix: let debug builds pin "now" (like `DEN_TEST_CONFIRM`), and set it in
  the test runner
- Test: the statusline and sync tests, once pinned

### B-007 · Security review of M5–M7: what is still open
- Found: 2026-09-24, an independent read-only review of locking, the
  agent, askpass, sync, nudges and downloads (16 findings; fixed ones are
  under Closed, B-008)
- Status: open, each needs the owner or is a known limit
- Touch ID is a check in den-agent's code, not a biometric keychain item:
  needs a Developer ID signature (TASKS.md, M7)
- Release downloads are checked against SHA256SUMS from the same host; a
  signature needs the owner's release key in `release/allowed_signers`
- While unlocked, software running as the person can read locked notes
  through the agent or Neovim's RPC socket (PLAN.md, "What locking does not
  do"); the time limit and idle forgetting bound it
- Decrypted text and passwords pass through ordinary (swappable) memory
- gpg-agent's pinentry can still appear for GPG-signed commits

## Closed

### B-009 · Bug hunt and second security review: fixed
- Found: 2026-09-24, an independent bug hunt (18 confirmed, 8 suspected)
  and a second read-only security review (6 findings)
- Status: closed
- Data loss or silent rewrites, fixed:
  - A move across two files could delete the task when the second file
    had changed on disk. Neovim now checks every change first (the buffer
    text, and the engine's new `check` for the disk), then applies them in
    the plan's order
  - Den could write into the wrong buffer: `bufnr()` matched names as a
    pattern. Buffers are now matched by their resolved path
  - An open, unsaved-free buffer was written over a file changed on disk
    since. The disk is checked first
  - Sync combined two different rewordings of a task into two tasks, and
    duplicated a task made only of tags. Such conflicts now wait for the
    person; added lines keep their place
  - Saving during a sync gave a hidden conflict with the sides swapped.
    Sync pulls with `--no-autostash` (committing and retrying if needed),
    reports any conflict left after a pull, and "mine" is always this
    machine's side on the conflict screen and in `take_side`
  - Mixed line endings were rewritten on any edit; each line now keeps
    its own
- Other fixes:
  - Moves left behind children after a blank line or indented with tabs
  - Words like `@done(soon)` were deleted when a task changed state
  - Captures could land inside an unclosed code fence or under a
    `### Someday` heading, out of every screen
  - Paused and locked projects counted in "doing" and overdue while the
    Tasks screen hid them; finished work still counts in Review
  - Sync always said "another sync is running" when `.git` is a file
    (worktrees, submodules)
  - New projects wrote `root:` unquoted, and could be made beside a locked
    project of the same name
  - Renamed or moved folders were never noticed; a watcher that failed to
    start was never reported
  - Symlinked folders duplicated notes, and symlink loops multiplied them
  - A locked note opened while the vault loaded showed the raw file:
    vault paths now work from the root alone
  - The Conflicts screen remembered a settled file for the whole session;
    `:Den inbox` kept an old project
  - Engine errors showed a Lua stack traceback; a snooze over 120 minutes
    did nothing; `>` from a project's note lost its running timer; the
    journal nag counted CRLF frontmatter as writing; a task ticked by hand
    stayed in the burndown forever; a level-1 heading did not end the Inbox
  - `:Den sync` and `den capture` now wait for unsaved Neovim buffers (each
    Neovim publishes its unsaved vault files for other writers)
  - Running setup twice could freeze Neovim: the old file watcher was
    stopped while holding the engine's lock its thread was waiting for
- Security review round 2, fixed:
  - HIGH: a synced symlink (`notes/x.md` → `.git/hooks/pre-commit`) let a
    Den write reach git's hooks or `.den/keys`. Writes and the vault scan
    refuse links that leave the visible vault
  - MEDIUM: other writers could not see Neovim's unsaved buffers (above);
    symlinked folder loops (above)
  - LOW: the release workflow could write to the repository in every job,
    and actions were pinned by tag. Only the publish job can write now,
    actions are pinned to commits, and checkouts keep no token
  - LOW: den-mcp's `journal` returned pages of any size; capped like
    `read_note`
  - INFO: `read_locked_note` ignores strict mode on purpose; documented
    where the agent handles it
- Test: regression tests beside each fix (Rust unit and integration tests,
  and tests/nvim/test_buffers, test_screens, test_sync, test_review)

### B-008 · Security review of M5–M7: fixed
- Found: 2026-09-24, same review as B-007
- Status: closed
- Fixed, each with a test where one can be written:
  - Any program running as the person could add a password or a YubiKey
    while the vault was unlocked, for lasting access: adding a way in now
    needs the current password or recovery key (`a_new_password_needs_the_vault_unlocked`)
  - An unlock could be kept alive forever by steady use: a hard limit,
    `lock.max_hours` (`an_unlock_ends_after_its_time_limit_even_when_used`)
  - Clients trusted whatever listened on the agent's socket, so a stand-in
    could collect a password; on Linux the socket folder was named from
    `$HOME`'s owner: clients now check the folder is private, the listener
    runs as the person and is den-agent itself, and the name uses the real
    user id (`clients_refuse_anything_but_den_agent_on_the_socket`)
  - A replaced `.den/keys/recipient` from the remote would have had new
    locked notes encrypted to someone else: each clone pins the key at
    unlock and refuses on a mismatch (`a_swapped_public_key_stops_locking_in_a_git_clone`)
  - A locked journal page got a plain page opened beside it, and locked
    folders did nothing: pages and notes are created locked in `.den-locked`
    folders and the locked page is the one opened
    (`a_locked_folder_makes_new_pages_locked_and_a_locked_page_is_not_shadowed`)
  - Nudges could be switched off without the seal: extreme settings, an
    endless snooze in the answers file, and deleting or backdating the
    "back on" file; settings are clamped, snoozes capped, and the back-on
    file now lives in root's folder and is compared by change time
    (`extreme_settings_and_endless_snoozes_do_not_switch_nudges_off`,
    `turning_nudges_back_on_needs_no_password_and_cannot_be_undone`)
  - Symlinks from a synced vault could make Den read or write outside it:
    writes resolve to real paths inside the real vault, and the scan skips
    links that leave (`symlinks_out_of_the_vault_are_refused`,
    `the_vault_does_not_follow_links_out_of_itself`)
  - `git log -p` by any tool showed decrypted notes: diffs show them only
    with `DEN_SHOW_LOCKED=1`
  - The agent inherited its first caller's environment (PATH for the
    YubiKey plugin): it starts with a small fixed one
  - The agent chmodded its socket folder before checking it; now it checks
    first and creates it private
  - Command and input history could reach ShaDa, and askpass typed some
    prompts in the clear: both closed
  - Temporary files were readable by others for a moment and could be
    committed mid-write; a slow sync could lose its lock to a second one
  - Downloads could leave a mix of versions and followed any redirect: all
    files are staged first, redirects are HTTPS only
  - Programs are signed with the hardened runtime; docs no longer claim
    screen-lock forgetting or keychain-enforced Touch ID

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
