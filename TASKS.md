# Tasks

Written in Den's own task syntax so Den can read this file once it exists:
`- [ ]` open, `- [/]` doing, `- [-]` dropped, `- [x]` done, `@done(date)` when
finished. Milestones and exit criteria are in [PLAN.md](PLAN.md).

## Inbox

## M0 · Fresh start

- [x] Work on a branch (claude/app-design-discussion-96381b) instead of main #git @done(2026-09-24)
- [x] Remove the old app: `crates/den-gui`, `crates/den-core`, `crates/den-nvim`, `demo/`, `design/`, `package.py`, `assets/fonts` #cleanup @done(2026-09-24)
- [x] Keep `assets/icons` (the ember bear) #decision @done(2026-09-24)
- [x] New workspace: `crates/den-core`, `crates/den-nvim`, `crates/den-cli`, edition 2024, workspace lints (`clippy -D warnings`) #setup @done(2026-09-24)
- [x] Pin the toolchain with `rust-toolchain.toml` #setup @done(2026-09-24)
- [x] GitLab CI: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` on Linux (shared runners have no macOS; run locally) #setup @done(2026-09-24)
- [x] Fixture vault at `tests/vault/` covering every rule in PLAN.md's format table, including edge cases (fences, bad dates, missing title, no frontmatter) #tests @done(2026-09-24)

## M1 · Engine: read

- [x] Vault discovery: `projects/`, `notes/`, `daily/`, `templates/` under a configurable root #engine @done(2026-09-24)
- [x] Frontmatter: parse only the block between the first two `---` lines; keep unknown keys #engine @done(2026-09-24)
- [x] Title: first `# ` heading, else the file name without `.md` #engine @done(2026-09-24)
- [x] Fence tracking for ``` and ~~~; nothing inside is parsed #engine @done(2026-09-24)
- [x] Task lines: bullets, states `[ ] [/] [-] [x]`, `#tag`, `@due`, `@done`; invalid dates reported, not dropped #engine @done(2026-09-24)
- [x] Record which section each task sits under (Inbox, Next actions, other) #engine @done(2026-09-24)
- [x] Buffer overlay: accept unsaved buffer text in place of the file on disk #engine @done(2026-09-24)
- [x] Project ↔ folder: match a directory to `root:`; resolve git worktrees to their main repository #engine @done(2026-09-24)
- [x] Queries: tasks (one project / all), inbox (one / all), one project, statusline summary #engine @done(2026-09-24)
- [x] Report unreadable and non-UTF-8 files instead of skipping them silently #engine @done(2026-09-24)
- [x] Watch the vault with `notify` and reload only changed files #engine @done(2026-09-24)
- [x] Snapshot tests over the fixture vault with `insta` #tests @done(2026-09-24)
- [x] Measure load time on a synthetic 5,000-note vault and record it in LOGS.md #tests @done(2026-09-24)

## M2 · Engine: write

- [x] Atomic writer: temp file in the same folder, fsync file and folder, rename, keep permissions, write through symlinks to the real file #engine @done(2026-09-24)
- [x] Stale check: refuse a write when the file changed since it was read #engine @done(2026-09-24)
- [x] Refuse files that have unsaved changes in a Neovim buffer #engine @done(2026-09-24)
- [x] Task operations: capture, start, pause, done (adds `@done`), drop, reopen (removes `@done`) #engine @done(2026-09-24)
- [x] Move a task to another project or section; Inbox → Next actions #engine @done(2026-09-24)
- [x] Create: new project with `root:`, link a folder, new note, today's journal page from the template #engine @done(2026-09-24)
- [x] Byte-exact tests for every operation #tests @done(2026-09-24)

## M3 · Neovim plugin

- [ ] Engine as a Lua module (`require("den_core")`); install builds or downloads the binary like blink.cmp #nvim
- [ ] `:Den <sub>` commands and a Lua API #nvim
- [ ] Background thread + pipe wake-up for vault loading #nvim
- [ ] Tasks screen `den://tasks`: this project ↔ all projects; `s p x - a o /` #nvim
- [ ] Inbox screen `den://inbox`: `m > - e o`, skim picker for `m` #nvim
- [ ] Capture float: project from the folder, `tab` to change #nvim
- [ ] New-folder prompt, asked once per folder: create / link / ignore #nvim
- [ ] Linked notes line under a project title; `:Den notes` #nvim
- [ ] markview: confirm `[/]` and `[-]` render; draw `@due` as "28 Sep · in 4 days" #nvim
- [ ] fzf-lua sources for projects, notes and tasks; live grep limited to the vault #nvim
- [ ] `Den*` highlight groups linked to standard groups #nvim

## M4 · Timer and `den`

- [ ] Timer log `.den/log/<machine>.jsonl`: start, pause, stop, rename #engine
- [ ] One running timer; starting a task marks it `[/]` #engine
- [ ] Follow renames made inside Neovim so timer history stays attached #engine
- [ ] Statusline component: timer + at most two insights, in priority order #nvim
- [ ] `den` command: `prompt`, `capture`, `status` #cli
- [ ] starship module example; keep `den prompt` to a few milliseconds #cli

## M5 · Sync

- [ ] `den init`: set up a vault repository with `.gitignore` for the index #sync
- [ ] Commit after ~30 s without changes #sync
- [ ] `pull --rebase` and `push` on start and on an interval #sync
- [ ] `den askpass`: hidden passphrase input inside Neovim #sync
- [ ] Background sync never prompts: `sync paused · key locked` #sync
- [ ] Conflicts: stop, explain in task terms, offer combine / keep either / by hand #sync
- [ ] gitoxide reads: changes waiting to sync, capture age from history #sync

## M6 · Review, journal, nudges

- [ ] Review screen with block-character charts #review
- [ ] Charts and focus rings as images through kitty (resvg), text fallback #review
- [ ] Journal page from template with the day's facts as virtual lines #journal
- [ ] One late-day journal line in the statusline #journal
- [ ] Break nudge: chair time, sunset, `w z q` #nudges
- [ ] Nudges off: fingerprint / password only, no agent tool, rotating messages #nudges

## M7 · Locking

- [ ] Vault key with one wrapped copy per unlock method #lock
- [ ] den-agent: user-only socket, peer check, key never leaves, memory locked and wiped, forgets on idle / sleep / screen lock #lock
- [ ] Unlock: Touch ID, Linux password prompt, YubiKey, password, recovery key #lock
- [ ] Lock and unlock notes; locked folders; `den lock` #lock
- [ ] Memory-only buffers for locked notes #lock
- [ ] git diff and merge helpers for `.md.age` #lock
- [ ] No task text in timer log lines for locked notes #lock
- [ ] Strict mode: one touch per Neovim session #lock

## M8 · Desktop

- [ ] Design the desktop app on the canvas: same features, its own UI #desktop
- [ ] Markdown editor component #desktop

## M9 · Agent

- [ ] MCP server: read tools and safe write tools through the engine #agent
- [ ] Read-only nudge status, no off switch #agent
- [ ] Locked notes only through den-agent, with confirmation #agent
- [ ] Local embeddings with sqlite-vec, if search by meaning turns out to be missing #agent

## Spikes

- [ ] Does iPhone step data reach the Mac through HealthKit? #spike
- [ ] kitty image placement: snacks.nvim image module or Den's own code #spike
- [ ] Dioxus native maturity before M8 #spike
- [ ] Which statusline plugins to support first #spike
