# Den

A quiet place for projects, notes and next actions: plain Markdown in one git
repository, a Rust engine that owns every rule about it, and Neovim as the
first way in. Den keeps time, keeps a journal, and nags you to go outside.

The plan, decisions and progress live in [PLAN.md](PLAN.md),
[TASKS.md](TASKS.md) and [LOGS.md](LOGS.md).

> Den replaces the older den.nvim plugin and uses the same `den` module name.
> Remove den.nvim before installing this one.

## Install

Neovim 0.11 or later (LuaJIT) and a Rust toolchain.

```lua
-- Neovim 0.12, built-in package manager
vim.pack.add({ "https://gitlab.com/cguz109/Den" })
require("den").setup({ vault = "~/Notes/den" })
```

Then build the engine (and the `den` command it uses for passphrase prompts)
once, and after each update:

```vim
:Den build
```

or from a shell, in the plugin's folder: `scripts/build-nvim.sh`.

The `den` command for the shell:

```sh
cargo install --path crates/den-cli
```

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
den init ~/Notes/den --remote git@gitlab.com:you/notes.git   # a new vault
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

`location` gives sunset nudges. Turning break nudges off is not a setting:
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
