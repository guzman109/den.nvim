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

None yet: the rebuild has no code.

## Closed

None yet.

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
