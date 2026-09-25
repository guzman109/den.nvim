# Den — plan

Den is a quiet place for projects, notes and next actions, kept as plain
Markdown in one git repository. A Rust engine owns every rule about that
vault. Neovim is the first interface, a small `den` command serves the shell
prompt and sync, a desktop app comes later, and an AI agent comes last. Den
also keeps time, keeps a journal, and nags you to go outside.

Screens for the Neovim version are drawn on the design canvas:
https://claude.ai/artifact/ADbMiGmm9Mve26G3d2ZPYF (private).

Work queue: [TASKS.md](TASKS.md) · Rules: [CONVENTIONS.md](CONVENTIONS.md) ·
History of decisions: [LOGS.md](LOGS.md) · Bugs: [BUGS.md](BUGS.md)

---

## Principles

1. **Markdown is the truth.** Everything Den shows is rebuilt from files in the
   vault. Anything else is a log or a cache that can be deleted.
2. **The engine returns data, never text for a screen.** Neovim draws it on a
   grid, the desktop app draws it however it likes.
3. **Same features everywhere, not the same UI.** Interfaces share the
   engine's actions and queries. The desktop app gets its own design.
4. **Synchronous core, async only at the edges.**
5. **Nothing slow on Neovim's main thread.**
6. **One write path. Never lose a byte.**
7. **Quiet by default.** Every screen, line and statusline item earns its place.

## Architecture

```
            ┌───────────── den-core (Rust, synchronous) ─────────────┐
            │ vault · parser · queries · operations · timer log       │
            │ git reads (gitoxide) · git writes (git CLI) · atomic IO │
            └─────┬──────────────┬───────────────┬──────────────┬─────┘
                  │              │               │              │
             den-nvim        den-cli        den-desktop      den-mcp
          (mlua cdylib,   (`den` binary:    (Dioxus, M8)    (agent, M9)
           Lua screens)    prompt, sync,
                           askpass)
                                  den-agent (M7): holds the unlocked key
```

- **den-core** — the engine. Knows nothing about windows, grids or editors.
- **den-nvim** — the engine loaded into Neovim as a Lua module (the way
  blink.cmp ships its matcher), plus the Lua that draws Den's screens.
- **den-cli** — the `den` command: prompt summary for starship, capture from
  the shell, sync, and the SSH passphrase helper.
- Later: **den-agent** (locking), **den-desktop** (Dioxus), **den-mcp** (agent).

## The vault

```
~/Notes/den/                    one git repository
  projects/<name>.md            one file per project: fields, Inbox, Next actions
  notes/<name>.md               everything else; joins a project with `project:`
  daily/<YYYY-MM-DD>.md         journal, one page per day (locked by default, M7)
  templates/daily.md            the journal template, editable
  inbox.md                      captures made outside any project
  .den/log/<machine>.jsonl      timer sessions, one append-only file per machine
  .den/index.sqlite             (later) search cache; gitignored, rebuilt anytime
```

Code repositories are never touched. A project points at its code folder with
`root:`. Neovim opened in that folder (or any git worktree of it) shows only
that project; the desktop app, the Tasks screen's "all projects" view and the
Inbox show everything.

## File format

```markdown
---
root: ~/Projects/Personal/website
status: active
due: 2026-10-01
---
# Personal website

## Inbox

- [ ] Find the old logo files

## Next actions

- [/] Draft the homepage story #writing @due(2026-09-28)
- [ ] Choose three projects to feature #writing
- [-] Build a carousel
- [x] Register the domain @done(2026-09-10)
```

| Piece | Rule |
|---|---|
| Fields | YAML frontmatter, only at the very top of the file |
| Project fields | `root`, `status` (`active` / `paused` / `archived`), `due` (turns on the burndown), `created` |
| Note fields | `project` (optional) |
| Daily fields | `date`, `mood` (optional) |
| Title | the first `# ` heading; the file name if there is none |
| Sections | `## Inbox` receives captures, `## Next actions` holds planned work; other headings are free |
| Task | a list item `- [ ]` open, `- [/]` doing, `- [-]` dropped, `- [x]` done |
| Order | line order in the file |
| Tags | `#tag` — starts with a letter; letters, digits, `-`, `_` |
| Dates | `@due(YYYY-MM-DD)`; `@done(YYYY-MM-DD)` is added by Den when a task is finished and removed when reopened |
| Code | nothing inside a fenced code block is parsed |
| Locked note (M7) | stored as `<name>.md.age` |

The syntax was chosen to match what markview.nvim already draws (frontmatter
properties, `[/]` and `[-]` checkboxes, `#tag` pills), so notes look right in
Neovim without Den. Den adds only what markview does not know, such as `@due`.

### Where each fact lives

| Fact | Stored in |
|---|---|
| Tasks, notes, projects | Markdown |
| When a task was finished | `@done(...)` in the line |
| Timer sessions | `.den/log/<machine>.jsonl` |
| When a capture arrived ("2h ago") | git history |
| Search index (later) | `.den/index.sqlite` |

## Sync

- **Automatic.** Den commits after about 30 seconds without changes
  (`den: <machine>, 3 changes`). When Neovim starts and every few minutes it
  runs `git pull --rebase`, then `git push`.
- **Writes use the `git` command**, so SSH config, keys, ssh-agent, credential
  helpers and encryption tools (git-crypt, git-remote-gcrypt) all work.
  **Reads use gitoxide**: status, ahead/behind, history, blame.
- **Passphrases.** Den ships an askpass helper that asks inside Neovim with a
  hidden input. Background sync never prompts; it pauses and the statusline
  says `sync paused · key locked`. `:Den sync` runs in the foreground and asks.
- **Conflicts.** Sync stops and leaves files alone; the statusline says
  `sync conflict · <file>`. Den explains the conflict in task terms ("this
  machine finished it, the other started it") and offers to combine, keep
  either side, or fix by hand.
- **Hash.** SHA-1 for now. Converting to SHA-256 later is cheap because nothing
  depends on the notes' commit hashes.

## Threading

The engine uses ordinary blocking file IO. The rule is where slow work runs:

| Operation | Roughly | Runs on |
|---|---|---|
| Re-read a file on save | < 1 ms | Neovim's main thread |
| Append a timer line, write one file | < 1 ms | main thread |
| Load the whole vault at startup | tens of ms | a background thread; screens appear when ready |
| Draw a chart or the focus rings | a few ms | a background thread |
| `git pull` / `git push` | seconds | a `git` process Neovim watches without waiting |
| Fingerprint or passphrase | as long as you take | den-agent / the askpass helper |

Background threads never touch Lua. They store their result and write one byte
to a pipe Neovim is watching; Lua wakes up and collects it.

## Features

### Capture and inbox
- Capture from any buffer. The project comes from the folder Neovim is in;
  `tab` picks another, `enter` saves, `esc` cancels. Outside a project it goes
  to a general inbox.
- The Inbox screen lists unsorted captures from every project with their age.
  `m` move to another project (skim picker), `>` make it a next action here,
  `-` drop, `e` edit, `o` open the file.

### Tasks
- States change through Den so side effects happen: `s` start (marks `[/]`,
  starts the timer), `p` pause, `x` done (adds `@done`), `-` drop, reopen.
- The Tasks screen shows this project, or all projects with `<Tab>`. `a` adds,
  `o` opens the line in its file, `f` filters, `g?` lists every key.
- Anything you can type — tags, due dates, reordering, renaming — you just
  type. Den adds actions only where typing would be awkward.

### Timer and focus rings
- One timer runs at a time; starting another pauses the first. Doing (`[/]`)
  and timing are different: several tasks can be in progress, one is timed.
- The timer lives in the statusline, so it follows you into every buffer.
- Focus rings (kitty): outer ring the current session (default 50 min),
  middle ring today's focus against a daily goal (default 4 h), inner ring
  steps toward 10,000 when step data exists. Redrawn once a second.

### Statusline and prompt
- Timer, plus at most two insights, most urgent first: break nudge, overdue,
  due today, behind pace, inbox count, done today. Sync state appears only
  when something is waiting or wrong.
- `den prompt` prints the same summary for the shell prompt (starship).

### Review
- Closed per day (from `@done`), a burndown for projects with an end date,
  progress per project, time this week (from the timer log).
- Real charts through the kitty image protocol; block-character charts
  everywhere else.

### Journal
- `daily/<date>.md` from `templates/daily.md`: *On my mind*, *Went well*,
  *Tomorrow*, optional `mood`.
- The day's facts (what you worked on, finished, where time went) are drawn as
  virtual lines, never written into the file.
- One gentle late-day statusline line (`journal · not written today`). No
  streak counters.

### Break nudges
- After a long stretch in the chair, a small window that does not take focus:
  time in the chair, time until sunset, steps if available. `w` walk (counts
  as a break), `z` snooze 30 min, `q` not today.
- Sunset is calculated offline from a configured location.
- **Turning nudges off is for humans only.** No agent tool for it, not stored
  in an editable config file, and `:Den nudges off` asks for a fingerprint
  (or password / YubiKey) behind a dialog with rotating guilt-trip messages.

### Locking (M7)
- Lock individual notes or projects; a folder can make new notes locked.
- One vault key encrypts locked notes; each unlock method keeps its own
  wrapped copy of that key: Touch ID (macOS), system password prompt with
  optional fingerprint (Linux), YubiKey, password, recovery key.
- den-agent holds the unlocked key for the session, over a user-only socket.
  The key never leaves it; it forgets on idle, sleep, screen lock or
  `den lock`. Strict mode: one touch per Neovim session.
- Locked notes open into memory-only buffers (no swap, no undo files). Git
  gets diff and merge helpers. Timer log lines for locked notes store no text.

### Setup, notes and search
- Opening Neovim in a folder that is not a project asks once: create, link to
  an existing project, or ignore.
- A project's notes appear as a line under its title; `:Den notes` lists them.
- Find by name through fzf-lua (skim); search inside notes with ripgrep,
  limited to the vault.

## Milestones

| | Milestone | Done when |
|---|---|---|
| M0 | Fresh start | Old crates removed on a branch; new workspace builds; CI runs fmt, clippy, tests; fixture vault exists |
| M1 | Engine: read | The fixture vault parses to the expected data (snapshot tests); project ↔ folder matching works for worktrees |
| M2 | Engine: write | Every task operation produces byte-exact output; atomic writes keep permissions and symlinks |
| M3 | Neovim plugin | Tasks, Inbox, capture and the new-folder prompt work in Neovim against a real (test) vault |
| M4 | Timer and `den` | Timer in the statusline; `den prompt` returns in a few ms; starship shows it |
| M5 | Sync | Two machines (or two clones) edit and sync without losing anything; conflicts are explained |
| M6 | Review, journal, nudges | Review with kitty charts, journal pages with day facts, break nudges with the human-only off switch |
| M7 | Locking | Lock/unlock with Touch ID, YubiKey and password; nothing unencrypted reaches disk or logs |
| M8 | Desktop | A Dioxus app, designed separately, over the same engine |
| M9 | Agent | An MCP server exposing Den's actions safely |

## Dependencies

**M0–M6 (engine, Neovim, `den`)**

| Crate | For |
|---|---|
| `jiff` | dates, local day, time zones |
| `serde`, `serde_json` | timer log, data handed to Lua |
| `serde_norway` | YAML frontmatter (`serde_yaml` is deprecated) |
| `gix` | git reads: status, ahead/behind, history, blame |
| `notify` | watching the vault for outside changes |
| `thiserror` | error types |
| `gethostname` | the default machine name for the timer log |
| `sunrise` | sunset time, offline |
| `resvg` | charts and rings drawn as SVG, turned into images |
| `base64` | kitty image protocol |
| `mlua` (`module`, `luajit`, `serialize`) | the engine inside Neovim |
| `clap` | the `den` command |
| `rmpv` | the askpass helper talking to Neovim |

Tests: `tempfile`, `insta`.

Programs: git, OpenSSH, Neovim ≥ 0.11 (LuaJIT), ripgrep, fzf-lua with skim;
optional markview.nvim and kitty.

**Later:** `age` (+ `age-plugin-yubikey` program), `secrecy`, `zeroize`, `nix`,
`robius-authentication`, `security-framework`, `keyring`, `zbus` (M7) ·
`objc2-health-kit` (steps, if it works) · `dioxus`, `nucleo-matcher`,
`pulldown-cmark` (M8) · `rusqlite` + `sqlite-vec` (search, only if needed) ·
`rmcp`, `tokio` (M9).

## Platforms

macOS and Linux. Images need a terminal with the kitty graphics protocol;
everything has a text fallback. On macOS den-agent is code-signed with the
hardened runtime so its key can be sealed behind Touch ID.

## Decided against

| Idea | Why not |
|---|---|
| Tasks in a database or event log as the truth | Merges depend on clocks; git shows JSON instead of tasks; editing as text needs diff translation; logs grow forever; two formats |
| A database file synced through git | Binary: no diffs, no merges |
| neuxdb | One encrypted blob breaks git merges; exclusive lock blocks several processes; very young project |
| `@order`, `@id`, `@status` tokens | Machine data in notes; line order and `[/]` replace them |
| Locking the whole vault (incl. KDE Vault, disk images) | Breaks search and merges, all or nothing, no fingerprint, different per OS; per-note locks give finer control |
| Passkeys to unlock | Built for web sign-in; decrypting needs PRF, an owned domain and Apple frameworks; no store on Linux |
| SHA-256 repositories now | gitoxide lacks it, git2 only behind an unstable flag, major hosts don't serve it |
| git2 / libgit2 | The `git` command and gitoxide cover it; libssh2 ignores `~/.ssh/config`; no remote helpers |
| skim as a library | A full terminal app; the engine only needs a matcher (nucleo, later) |
| tokio in the engine | Local file IO is not faster async; a sync core composes with Lua, CLI, Dioxus and MCP |
| A desktop UI identical to Neovim | The desktop app gets its own design over the same features |
| A text message to stop agents turning nudges off | Agents treat file text as data; the rule is enforced in code |

## Open questions

- Does iPhone step data reach the Mac through HealthKit? (spike)
- kitty image placement: snacks.nvim's image module or Den's own code? (spike)
- How mature is Dioxus native by M8? (spike)
- Which statusline plugins to support first; Den exposes a component either way.
- Default thresholds: chair time before a nudge, session length, daily goal.
- Where the notes repository is pushed (matters for SHA-256 later).
