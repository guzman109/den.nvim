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

- [x] Prebuilt engine downloads: `scripts/package.sh`, CI for GitLab and GitHub, `:Den build` checks SHA256SUMS and an optional SSH signature #nvim @done(2026-09-24)
- [x] Host: GitHub, with CI and releases in GitHub Actions #release @done(2026-09-24)
- [ ] Create the GitHub repository, push, and tag the first release #release

- [x] Engine as a Lua module (`require("den_native")`); `:Den build` / `scripts/build-nvim.sh` builds it #nvim @done(2026-09-24)
- [x] `:Den <sub>` commands and a Lua API #nvim @done(2026-09-24)
- [x] Background thread + pipe wake-up for vault loading #nvim @done(2026-09-24)
- [x] Tasks screen `den://tasks`: this project ↔ all projects (`<Tab>`); `s p x - a e m o f g?` #nvim @done(2026-09-24)
- [x] Inbox screen `den://inbox`: `m > - e o`, skim picker for `m` #nvim @done(2026-09-24)
- [x] Capture float: project from the folder, `tab` to change #nvim @done(2026-09-24)
- [x] New-folder prompt, asked once per folder: create / link / ignore #nvim @done(2026-09-24)
- [x] Linked notes line under a project title; `:Den notes` #nvim @done(2026-09-24)
- [x] markview: `[/]` and `[-]` render by default; `@due` coloured with "in 4 days" after it #nvim @done(2026-09-24)
- [x] fzf-lua sources for projects, notes and tasks; live grep limited to the vault #nvim @done(2026-09-24)
- [x] `Den*` highlight groups linked to standard groups #nvim @done(2026-09-24)

## M4 · Timer and `den`

- [x] Timer log `.den/log/<machine>.jsonl`: start, pause, stop, rename #engine @done(2026-09-24)
- [x] One running timer; starting a task marks it `[/]` #engine @done(2026-09-24)
- [x] Follow renames made inside Neovim so timer history stays attached #engine @done(2026-09-24)
- [x] Statusline component: timer + at most two insights, in priority order #nvim @done(2026-09-24)
- [x] `den` command: `prompt`, `capture`, `status` #cli @done(2026-09-24)
- [x] starship module example; keep `den prompt` to a few milliseconds #cli @done(2026-09-24)

## M5 · Sync

- [x] `den init [--remote]`: set up a vault repository with `.gitignore` for the index #sync @done(2026-09-24)
- [x] Commit after ~30 s without changes #sync @done(2026-09-24)
- [x] `pull --rebase` and `push` on start and on an interval #sync @done(2026-09-24)
- [x] Askpass: hidden passphrase input inside Neovim (`den` over Neovim's socket) #sync @done(2026-09-24)
- [x] Background sync never prompts: `sync paused · key locked` #sync @done(2026-09-24)
- [x] Conflicts: combine task edits that do not overlap; otherwise stop and offer combine / keep either / both / by hand #sync @done(2026-09-24)
- [x] gitoxide reads: changes waiting to sync, capture age from history #sync @done(2026-09-24)
- [x] Syncs wait while a vault buffer has unsaved edits; a detached `den sync` sends what is left on quit #sync @done(2026-09-24)
- [ ] Try sync against a real remote on two machines, with a locked SSH key #sync

## M6 · Review, journal, nudges

- [x] Review screen with block-character charts #review @done(2026-09-24)
- [x] Charts and focus rings as images through kitty (resvg), text fallback #review @done(2026-09-24)
- [x] Journal page from template with the day's facts as virtual lines #journal @done(2026-09-24)
- [x] One late-day journal line in the statusline #journal @done(2026-09-24)
- [x] Break nudge: chair time, sunset, `w z q` #nudges @done(2026-09-24)
- [x] Nudges off: OS password dialog writes a root-owned seal, no agent tool, rotating messages #nudges @done(2026-09-24)
- [ ] Look at the kitty images in a real kitty window (headless tests cannot) #review
- [ ] Nudges off: add a fingerprint / YubiKey signature once den-agent exists #nudges

## M7 · Locking

- [x] Vault key with one wrapped copy per unlock method #lock @done(2026-09-24)
- [x] den-agent: user-only socket, peer check, key never leaves, memory locked and wiped, forgets on idle and sleep #lock @done(2026-09-24)
- [x] Unlock: password, recovery key; YubiKey and Touch ID written #lock @done(2026-09-24)
- [x] Lock and unlock notes; `den lock` / `den unlock`; `:Den lock` / `:Den unlock` #lock @done(2026-09-24)
- [x] Memory-only buffers for locked notes (no swap, no undo file, no registers or search history in ShaDa) #lock @done(2026-09-24)
- [x] git diff and merge helpers for `.md.age` (decrypted in memory only) #lock @done(2026-09-24)
- [x] No task text in timer log lines for locked notes #lock @done(2026-09-24)
- [x] Strict mode: one unlock per Neovim session #lock @done(2026-09-24)
- [ ] Try Touch ID on the owner's Mac (needs a finger; never run by tests) #lock
- [ ] Try a YubiKey with age-plugin-yubikey (needs the key) #lock
- [ ] Sign den-agent with the owner's Developer ID, and make the Touch ID copy a biometric keychain item (B-007) #lock
- [ ] A release signing key: `release/allowed_signers` in the plugin, SHA256SUMS signed by the owner (B-007) #release
- [ ] Forget the key when the screen locks (sleep and idle already do) #lock
- [ ] Linux: a system password prompt (polkit) as an unlock method; for now YubiKey or password #lock
- [x] Locked folders: new notes and journal pages in a folder with `.den-locked` start locked (`den lock folder`) #lock @done(2026-09-24)
- [x] Fix the security review's findings (B-008); the rest are listed in B-007 #lock @done(2026-09-24)

## M8 · Desktop

- [ ] Design the desktop app on the canvas: same features, its own UI #desktop
- [ ] Markdown editor component #desktop

## M9 · Agent

- [ ] MCP server: read tools and safe write tools through the engine #agent
- [ ] Read-only nudge status, no off switch #agent
- [ ] Locked notes only through den-agent, with confirmation #agent
- [ ] Local embeddings with sqlite-vec, if search by meaning turns out to be missing #agent

## Spikes

- [x] Does iPhone step data reach the Mac through HealthKit? No: a steps file from an iOS Shortcut instead #spike @done(2026-09-24)
- [x] kitty image placement: snacks.nvim image module or Den's own code #spike @done(2026-09-24)
- [x] Dioxus native maturity before M8: not ready for an editor; webview renderer + CodeMirror 6 recommended (owner decides) #spike @done(2026-09-24)
- [ ] Which statusline plugins to support first #spike
