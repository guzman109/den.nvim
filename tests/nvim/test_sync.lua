-- Sync from Neovim, against a real git remote: the vault is one clone, a
-- second clone plays the other machine. Git runs without the machine's own
-- config, so signing and hooks cannot change the result.

local native = require("den.native")
local sync = require("den.sync")
local statusline = require("den.statusline")

for k, v in pairs({
  GIT_CONFIG_GLOBAL = "/dev/null",
  GIT_CONFIG_NOSYSTEM = "1",
  GIT_AUTHOR_NAME = "Den Test",
  GIT_AUTHOR_EMAIL = "den@example.invalid",
  GIT_COMMITTER_NAME = "Den Test",
  GIT_COMMITTER_EMAIL = "den@example.invalid",
}) do
  vim.env[k] = v
end

local function git(dir, args, env)
  local cmd = { "git" }
  vim.list_extend(cmd, args)
  local res = vim.system(cmd, { cwd = dir, text = true, env = env }):wait()
  if res.code ~= 0 then
    error("git " .. table.concat(args, " ") .. ": " .. (res.stderr or ""), 2)
  end
  return vim.trim(res.stdout or "")
end

local base = T.tmp .. "/sync"
local remote = base .. "/remote.git"
local other = base .. "/other"
vim.fn.mkdir(remote, "p")
git(remote, { "init", "-q", "--bare", "-b", "main" })

-- The vault's history starts three days ago, so its captures have an age.
local three_days_ago = os.date("!%Y-%m-%dT%H:%M:%SZ", os.time() - 3 * 86400)
git(T.vault, { "init", "-q", "-b", "main" })
git(T.vault, { "add", "-A" })
git(T.vault, { "commit", "-qm", "start" }, { GIT_AUTHOR_DATE = three_days_ago, GIT_COMMITTER_DATE = three_days_ago })
git(T.vault, { "remote", "add", "origin", remote })
git(T.vault, { "push", "-q", "-u", "origin", "main" })
git(base, { "clone", "-q", remote, "other" })

-- Edits start a sync after a quiet period; here it is long enough never to
-- fire on its own, except in the test about it.
local settings = require("den.state").info.config.sync
settings.commit_after_seconds = 3600

local function write(path, text)
  local f = assert(io.open(path, "w"))
  f:write(text)
  f:close()
end

--- Waits until the running sync has finished.
local function finished()
  T.settle(50)
  T.ok(
    vim.wait(10000, function()
      local s = native.call("sync_status")
      return s and not s.running
    end, 20),
    "the sync finishes"
  )
  T.settle(100)
  return native.call("sync_status")
end

T.test("an edit goes out once the vault is quiet", function()
  local before = native.call("sync_status").at
  settings.commit_after_seconds = 0.3
  T.ok(require("den.apply").run(function(mod)
    return mod.plan_capture("website", "Call the framer")
  end))
  T.ok(
    vim.wait(10000, function()
      local s = native.call("sync_status")
      return s.at ~= before and not s.running
    end, 20),
    "a sync ran by itself"
  )
  settings.commit_after_seconds = 3600
  local s = finished()
  T.eq(s.outcome, { kind = "synced", committed = 1, pulled = false, pushed = true, combined = 0 })
  T.eq(git(remote, { "log", "-1", "--format=%s" }), "den: test, 1 change")
  statusline.refresh()
  T.lacks(statusline.text(), "sync", "a healthy sync says nothing")
end)

T.test("with nothing waiting, the quiet-period sync skips the network", function()
  local before = native.call("sync_status").at
  T.ok(sync.background({ if_waiting = true }))
  local s = finished()
  T.eq(s.at, before, "no sync ran")
end)

T.test("the inbox shows how long each capture has waited", function()
  require("den").inbox()
  T.settle()
  local lines = table.concat(T.lines(), "\n")
  T.ok(lines:match("Find the old logo files%s+3d"), lines)
  -- Committed a moment ago.
  T.ok(lines:match("Call the framer%s+%d+m"), lines)
end)

T.test("askpass asks inside Neovim and hands the answer to SSH", function()
  local program = sync.program()
  T.ok(program, "bin/den is built")
  if vim.v.servername == "" then
    vim.fn.serverstart()
  end
  local asked
  local real = vim.fn.inputsecret
  vim.fn.inputsecret = function(opts)
    asked = opts.prompt
    return "correct horse"
  end
  local res
  vim.system({ program, "Enter passphrase for key '/k/id_ed25519': " }, {
    text = true,
    env = { DEN_ASKPASS = "1", DEN_NVIM = vim.v.servername },
  }, function(r)
    res = r
  end)
  vim.wait(5000, function()
    return res ~= nil
  end, 20)
  vim.fn.inputsecret = real
  T.ok(res, "askpass returns")
  T.eq(res.code, 0)
  T.eq(res.stdout, "correct horse\n")
  T.eq(asked, "Den · Enter passphrase for key '/k/id_ed25519': ")
end)

T.test("cancelling the question tells SSH no", function()
  local program = sync.program()
  local real = vim.fn.inputsecret
  vim.fn.inputsecret = function(opts)
    return opts.cancelreturn
  end
  local res
  vim.system({ program, "Enter passphrase: " }, {
    text = true,
    env = { DEN_ASKPASS = "1", DEN_NVIM = vim.v.servername },
  }, function(r)
    res = r
  end)
  vim.wait(5000, function()
    return res ~= nil
  end, 20)
  vim.fn.inputsecret = real
  T.eq(res and res.code, 1)
  T.eq(res.stdout, "")
end)

T.test("a conflict opens the conflict screen, and settling it finishes the sync", function()
  git(other, { "pull", "-q" })
  local theirs = io.open(other .. "/projects/website.md"):read("*a")
  write(other .. "/projects/website.md", (theirs:gsub("No hero video%.", "No hero video, ever.")))
  git(other, { "commit", "-qam", "den: laptop, 1 change" })
  git(other, { "push", "-q" })

  local path = T.vault .. "/projects/website.md"
  local mine = io.open(path):read("*a")
  write(path, (mine:gsub("No hero video%.", "One short hero video.")))
  native.call("reload", "projects/website.md")

  T.ok(sync.run(), "a foreground sync starts")
  local s = finished()
  T.eq(s.outcome, { kind = "conflict", files = { "projects/website.md" } })
  T.contains(T.notes[#T.notes], "both machines changed the same lines in projects/website.md")
  T.eq(vim.api.nvim_buf_get_name(0), "den://conflicts")
  local shown = T.lines()
  T.contains(shown, "1 to settle")
  T.contains(shown, "other machine (laptop)")
  T.contains(shown, "Keep the writing plain. No hero video, ever.")
  T.contains(shown, "this machine")
  T.contains(shown, "Keep the writing plain. One short hero video.")
  statusline.refresh()
  T.contains(statusline.text(), "sync conflict · :Den sync")

  -- Combining is refused: both sides reworded the same sentence.
  for row, line in ipairs(shown) do
    if line:find("projects/website.md · line", 1, true) then
      vim.api.nvim_win_set_cursor(0, { row, 0 })
    end
  end
  vim.cmd("normal c")
  T.contains(T.notes[#T.notes], "these edits overlap")

  vim.cmd("normal b")
  local s2 = finished()
  T.eq(s2.outcome.kind, "synced")
  T.ok(s2.outcome.pushed, "pushed")
  T.contains(T.read("projects/website.md"), "No hero video, ever.\nKeep the writing plain. One short hero video.\n")
  T.lacks(T.read("projects/website.md"), "<<<<<<<")
  statusline.refresh()
  T.lacks(statusline.text(), "sync conflict")
  git(other, { "pull", "-q" })
  T.eq(io.open(other .. "/projects/website.md"):read("*a"), T.read("projects/website.md"))
end)

T.test("a locked key pauses the background sync and never asks", function()
  local record = base .. "/ssh-called"
  local fake = base .. "/fake-ssh"
  write(fake, ("#!/bin/sh\necho \"$@\" > '%s'\necho 'Permission denied (publickey).' >&2\nexit 255\n"):format(record))
  vim.uv.fs_chmod(fake, 493)
  git(T.vault, { "remote", "set-url", "origin", "ssh://git@example.invalid/vault.git" })
  git(T.vault, { "config", "core.sshCommand", fake })

  local real = vim.fn.inputsecret
  local asked = false
  vim.fn.inputsecret = function()
    asked = true
    return ""
  end
  T.ok(require("den.apply").run(function(mod)
    return mod.plan_capture("website", "Ring the printer")
  end))
  T.ok(sync.background())
  local s = finished()
  vim.fn.inputsecret = real
  T.eq(s.outcome.kind, "key_locked")
  T.ok(not asked, "nothing was asked")
  T.contains(io.open(record):read("*a"), "BatchMode=yes")
  statusline.refresh()
  T.contains(statusline.text(), "sync paused · key locked · 1 waiting")
  git(T.vault, { "remote", "set-url", "origin", remote })
  git(T.vault, { "config", "--unset", "core.sshCommand" })
end)

T.test("background syncs wait while a vault buffer has unsaved edits", function()
  vim.cmd.edit(T.vault .. "/projects/haste.md")
  vim.api.nvim_buf_set_lines(0, -1, -1, false, { "- [ ] Unsaved thought" })
  T.eq(sync.background(), false, "held back")
  vim.cmd("silent write")
  T.settle()
  T.ok(sync.background(), "goes once saved")
  local s = finished()
  T.eq(s.outcome.kind, "synced")
end)

T.test(":Den sync waits for unsaved vault files too", function()
  vim.cmd.edit(T.vault .. "/projects/haste.md")
  vim.api.nvim_buf_set_lines(0, -1, -1, false, { "- [ ] Another unsaved thought" })
  T.eq(sync.run(), false, "held back")
  T.contains(T.notes[#T.notes], "unsaved changes to projects/haste.md")
  vim.cmd("silent write")
  T.settle()
  T.ok(sync.run(), "goes once saved")
  local s = finished()
  T.eq(s.outcome.kind, "synced")
end)
