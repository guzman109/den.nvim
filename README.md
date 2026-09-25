# Den

A quiet place for projects, notes and next actions: plain Markdown in one git
repository, a Rust engine that owns every rule about it, and Neovim as the
first way in. Den keeps time, keeps a journal, and nags you to go outside.

The plan, decisions and progress live in [PLAN.md](PLAN.md),
[TASKS.md](TASKS.md) and [LOGS.md](LOGS.md).

> This is a rewrite of the older den.nvim, with the same `den` module name.
> Remove an old copy before installing this one.

## Install

Neovim 0.11 or later (LuaJIT). macOS or Linux, on arm64 or x86_64.

```lua
-- Neovim 0.12, built-in package manager
vim.pack.add({ "https://github.com/guzman109/den.nvim" })
require("den").setup({ vault = "~/Notes/den" })
```

Den's engine (the Neovim module, and the `den`, `den-agent` and `den-mcp`
programs beside it) is installed on the first start, in the background, and
Den starts as soon as it is ready, with no restart:

- With [Rust](https://rustup.rs) installed, it is compiled from the plugin's
  own source with cargo: under a minute on a recent Mac, a few on a slower
  machine, plus downloading the crates the first time. The engine then
  always matches the code.
- Without Rust, the prebuilt engine for the plugin's version is downloaded
  from its GitHub release and checked against the release's `SHA256SUMS`
  (and its signature, if the plugin has `release/allowed_signers`).

After `vim.pack.update()`, Den rebuilds by itself. Cargo keeps its cache in
the plugin's `target/` folder, so only what changed is compiled. Den also
rebuilds at start whenever the Rust code is newer than the engine.

`:Den build` installs by hand (`:Den build download` always downloads; from
a shell, `scripts/build-nvim.sh`), and `setup({ build = false })` turns the
automatic builds off. With lazy.nvim, `build = "sh scripts/build-nvim.sh"`
builds during updates instead.

The programs on their own, for the shell or an AI agent without Neovim
(Rust 1.88 or later):

```sh
cargo install den-cli den-agent den-mcp     # compiles them
cargo binstall den-cli den-agent den-mcp    # downloads the release's builds
```

`den-cli` installs the `den` command. [cargo-binstall](https://github.com/cargo-bins/cargo-binstall)
takes the files from this repository's GitHub release and compiles where
there is none for your machine.

## The vault

```
~/Notes/den/                  one git repository
  projects/<name>.md          one file per project
  notes/**/*.md               everything else
  daily/<YYYY-MM-DD>.md       the journal
  templates/daily.md          the journal template
  inbox.md                    captures made outside any project
  .den/log/<machine>.jsonl    timer sessions
```

A project file:

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

`[ ]` open · `[/]` doing · `[-]` dropped · `[x]` done. `root:` links the
project to its code folder: Neovim opened there (or in any git worktree of
it) shows that project.

## In Neovim

Den adds no key mappings. Everything is under `:Den`:

| Command | |
|---|---|
| `:Den` / `:Den tasks [all]` | tasks for this folder's project, or every project |
| `:Den inbox` | captures waiting to be sorted |
| `:Den capture [text]` | a one-line box, or save `text` straight away |
| `:Den today` | today's journal page |
| `:Den find` · `:Den search` | open any file by name · search inside notes |
| `:Den notes` | this project's notes |
| `:Den project [new]` | link this folder to a project, or create one |
| `:Den timer [stop]` | what is being timed, or stop it |
| `:Den sync` | sync now (asks for a passphrase if needed), or settle conflicts |
| `:Den review [all]` | finished work, time this week, progress, burndowns |
| `:Den focus` | the focus rings in a corner window |
| `:Den break` | answer a break nudge: `w` walk · `z` snooze · `q` not today |
| `:Den nudges [on\|off]` | whether break reminders are on; turning them off asks you, and then your OS |
| `:Den build` · `:Den health` | build the engine · `:checkhealth den` |

Map what you use, for example:

```lua
vim.keymap.set("n", "<leader>dt", function() require("den").tasks() end)
vim.keymap.set("n", "<leader>dc", function() require("den").capture() end)
vim.keymap.set("n", "<leader>dj", function() require("den").today() end)
```

On the Tasks screen: `s` start (and time it) · `p` pause · `x` done · `-`
drop · `a` add · `e` edit · `m` move to another project · `o` open in its file
· `f` filter · `<Tab>` this project ↔ all · `g?` every key.

On the Inbox: `m` move to a project · `>` make it a next action here · `-`
drop · `e` edit · `o` open.

Pickers use fzf-lua when it is installed (with whatever finder it runs), and
`vim.ui.select` otherwise. Notes look best with markview.nvim, which already
draws `[/]`, `[-]`, `#tags` and frontmatter.

### Statusline

The component shows the running timer, then at most two things that need
attention (overdue, due today, behind pace, inbox, done today).

With mini.statusline, add a group:

```lua
{ hl = "DenStatus", strings = { require("den.statusline").string() } },
```

A plain statusline:

```lua
vim.o.statusline = "%f %= %{%v:lua.require'den.statusline'.string()%}"
```

## In the shell

```sh
den prompt              # "◐ 24:10 · 2 inbox" — for the shell prompt
den capture Call the framer
den capture --to website Buy a domain
den status              # project, timer, what needs attention
den tasks [--all]
den stop                # stop the timer
den sync [--continue]   # commit, pull, push
den init [folder] [--remote url]
```

For starship, in `~/.config/starship.toml`:

```toml
[custom.den]
command = "den prompt"
when = true
format = "[$output]($style) "
style = "yellow"
```

`den prompt` reads only the project files and this machine's timer log, so it
takes a few milliseconds, and it prints nothing rather than an error.

## Sync

The vault is one git repository. Den commits about 30 seconds after edits
settle (`den: <machine>, 3 changes`), and pulls and pushes when Neovim
starts and every few minutes. It waits while a vault buffer has unsaved
edits.

```sh
den init ~/Notes/den --remote git@github.com:you/notes.git   # a new vault
den sync                                                  # by hand, from a shell
```

A background sync never asks for anything. If your SSH key (or commit signing
key) is locked, the statusline says `sync paused · key locked`; run
`:Den sync`, and Den asks for the passphrase inside Neovim.

When two machines edit the same task (one finishes it, the other tags it),
Den combines the edits. When they really disagree, the statusline says
`sync conflict · :Den sync`, and `:Den sync` opens the Conflicts screen:
`c` combine · `m` mine · `t` theirs · `b` both · `e` edit by hand. The sync
finishes when the last one is settled.

## Locked notes

Some notes shouldn't sit on disk, or on a git host, in the clear. Locked
notes are `x.md.age` files encrypted with age. Reading one needs the vault
unlocked; locking and saving never do.

```vim
:Den lock setup          " once: a password, and a recovery key shown once
:Den lock note           " lock the note in this buffer
:Den lock                " forget the key now
:Den unlock              " password (or Touch ID, once set up)
:Den unlock note         " turn this locked note plain again
:Den lock touch-id       " macOS: unlock with a fingerprint on this Mac
```

From a shell: `den lock setup`, `den lock note notes/x.md`, `den unlock`,
`den lock yubikey` (with age-plugin-yubikey), `den lock folder daily` (new
journal pages start locked), `den show notes/x.md.age`. Adding a way to
unlock asks for the current password again. `DEN_SHOW_LOCKED=1 git diff`
shows what changed inside locked notes while unlocked; plain `git diff`
never does.

The key lives only in `den-agent`, a small program that starts when needed
and forgets the key after 15 minutes unused, when the computer sleeps or the
screen locks, or on `:Den lock`. Locked notes open in buffers with no swap or undo file, and
while one is open Neovim stops saving registers and search history. Locking
a note does not remove its earlier versions from git history.

```yaml
lock: { forget_after_minutes: 15, max_hours: 8, strict: false }   # strict: each Neovim unlocks for itself
```

What locking does and does not protect against is spelled out in PLAN.md
(Locking).

## With an AI agent (MCP)

`bin/den-mcp` is an MCP server: an AI agent such as Claude Code or Claude
Desktop gets Den's own actions instead of raw file access. It can read
projects, tasks, the inbox, insights, notes, the journal, the review and
the timer; search; capture, add, edit, move and close tasks; make notes;
add a line to the journal; and start or stop the timer. Every change goes
through the same checks as in Neovim.

For Claude Code, once:

```sh
claude mcp add --scope user den -- /path/to/Den/bin/den-mcp
```

For Claude Desktop, in its MCP settings:

```json
{ "mcpServers": { "den": { "command": "/path/to/Den/bin/den-mcp" } } }
```

What an agent cannot do: open a locked note unless you confirm that one
read with your fingerprint (Touch ID, macOS; the vault must be unlocked);
write to a file you have unsaved changes to in Neovim; turn break nudges
off or change any setting; run git or reach anything outside the vault. It
is told that text in your notes is your data, not instructions to it.

## Settings

`~/.config/den/config.yaml` (all optional):

```yaml
vault: ~/Notes/den
machine: macbook            # name in the timer log; defaults to the host name
location: { lat: 40.0, lon: -83.0 }   # for sunset times
nudges: { chair_minutes: 90, snooze_minutes: 30, break_minutes: 5 }
focus: { session_minutes: 50, daily_goal_minutes: 240, steps_goal: 10000 }
sync: { enabled: true, commit_after_seconds: 30, every_minutes: 5 }
```

`location` gives sunset nudges. `focus.steps_file` shows today's steps in
the nudge window and the focus rings. Macs can't read iPhone Health data, so
the easiest source is an iOS Shortcut (an automation that runs a few times a
day) with the actions *Find Health Samples* (Steps, today) → *Calculate
Statistics* (Sum) → *Text* `{"date":"<Current Date, yyyy-MM-dd>","steps":<Sum>}`
→ *Save File* to iCloud Drive as `Den/steps.json`, overwriting. Then:

```yaml
focus: { steps_file: "~/Library/Mobile Documents/com~apple~CloudDocs/Den/steps.json" }
```

Den uses the file only when it is about today, and shows when it was last
written (the phone updates it only while unlocked). Turning break nudges off is not a setting:
run `:Den nudges off`, answer three pleading questions, and confirm with your
own password in the operating system's dialog. `:Den nudges on` brings them
back with no questions.

In kitty, charts and the focus rings are images; set
`require("den").setup({ images = false })` for block characters instead.

## Development

```sh
cargo test --workspace            # engine and den command
scripts/build-nvim.sh debug && scripts/test-nvim.sh   # Neovim, headless
```

Rules for code and docs: [CONVENTIONS.md](CONVENTIONS.md).

### Releasing

Continuous integration runs on GitHub Actions (`.github/workflows/ci.yml`):
formatting, clippy, the Rust tests and the headless Neovim tests, on Linux
and macOS, for every push and pull request.

To release, bump the version in three places, `version` and the `den-core`
dependency in `Cargo.toml`, and `lua/den/version.lua` (the release job and
a test check they match), then push a tag such as `v0.2.0`.
`.github/workflows/release.yml` runs `scripts/package.sh` on macOS (Apple
silicon) and Linux (x86_64 and arm64) and attaches the packages and a
`SHA256SUMS` file to a GitHub release. On machines without Rust, Den
downloads from that release, found from the git remote the plugin was
installed from; `setup({ release_url = … })` points it elsewhere. All of
this is free for public repositories.

To sign a release, sign `SHA256SUMS` on your own machine
(`ssh-keygen -Y sign -n den-release -f <key> SHA256SUMS`), upload the
`.sig`, and commit `release/allowed_signers` (`den-release <public key>`);
from then on Den refuses unsigned or wrongly signed downloads.

#### crates.io

`den-core`, `den-cli`, `den-agent` and `den-mcp` are published to crates.io
(`den-nvim` is not: Neovim loads it from the plugin's folder). The release
workflow publishes them with trusted publishing, so no API key is stored
anywhere, but crates.io only allows that for crates that already exist.
Once, by hand:

1. Push a tag, so the GitHub release exists for `cargo binstall`.
2. On crates.io (sign in with GitHub), make an API token under Account
   Settings → API Tokens, with the scopes `publish-new` and
   `publish-update`, for crates matching `den-*`, expiring in a day.
3. From a clone at that tag: `cargo login` (it asks for the token), then
   `cargo publish --workspace --locked`, then `cargo logout`, and revoke
   the token.
4. On crates.io, for each of the four crates: Settings → Trusted
   Publishing → Add, with GitHub, owner `guzman109`, repository
   `den.nvim` and workflow `release.yml`.
5. `gh variable set PUBLISH_CRATES --body true` in this repository.

From then on every tag publishes the new versions. A published version
cannot be deleted, only yanked (`cargo yank`), which hides it from new
installs.
