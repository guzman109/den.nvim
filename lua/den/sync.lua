-- Keeping the vault in step with its git remote, from Neovim.
--
-- Background syncs run on the engine's own thread and never ask for
-- anything: a sync that needs a passphrase stops and the statusline says
-- "sync paused · key locked". They run when Neovim starts, a short while
-- after edits settle (only if something is waiting), and every few minutes.
-- While a vault buffer has unsaved changes they wait, so a pull never lands
-- under an edit in progress.
--
-- `:Den sync` runs in the foreground: SSH may ask for a passphrase (inside
-- Neovim, through the `den` binary as askpass), the result is announced,
-- and a conflict opens the conflict screen.

local native = require("den.native")
local state = require("den.state")
local statusline = require("den.statusline")

local M = {}

local settle_timer, every_timer
local announce = false
local waiting_for_save = false
local edited_since_sync = false
local status = nil

local function settings()
  local config = state.info and state.info.config or {}
  return config.sync or {}
end

local function enabled()
  return settings().enabled ~= false
end

--- The `den` binary to use as askpass: the plugin's own build, else one on
--- PATH.
function M.program()
  local own = native.root .. "/bin/den"
  if vim.fn.executable(own) == 1 then
    return own
  end
  local found = vim.fn.exepath("den")
  return found ~= "" and found or nil
end

--- Whether any buffer holds unsaved changes to a vault file.
local function editing()
  for _, buf in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_loaded(buf) and vim.bo[buf].modified and vim.bo[buf].buftype == "" then
      -- The engine resolves links (a vault under /var is /private/var on macOS).
      if native.call("rel", vim.api.nvim_buf_get_name(buf)) then
        return true
      end
    end
  end
  return false
end

--- A sync nobody asked for: never prompts, skipped while editing.
function M.background(opts)
  if not enabled() or not state.ready then
    return false
  end
  if editing() then
    waiting_for_save = true
    return false
  end
  waiting_for_save = false
  return native.call("sync_run", { if_waiting = opts and opts.if_waiting or false, den = M.program() }) == true
end

--- `:Den sync`: may ask for a passphrase, says what happened.
function M.run(opts)
  opts = opts or {}
  local program = M.program()
  local started, err = native.call("sync_run", {
    askpass = program,
    den = program,
    server = program and vim.v.servername or nil,
    resume = opts.resume or false,
  })
  if err then
    vim.notify("Den: " .. err, vim.log.levels.ERROR)
    return false
  end
  if not started then
    vim.notify("Den: a sync is already running")
    return false
  end
  announce = true
  vim.cmd("redrawstatus")
  return true
end

--- Restarts the quiet-period timer; when it fires, sync if anything waits.
function M.after_edit()
  if not enabled() then
    return
  end
  edited_since_sync = true
  local seconds = settings().commit_after_seconds or 30
  if not settle_timer then
    settle_timer = vim.uv.new_timer()
  end
  settle_timer:stop()
  settle_timer:start(seconds * 1000, 0, vim.schedule_wrap(function()
    M.background({ if_waiting = true })
  end))
end

--- What a sync did, in words.
function M.describe(outcome)
  if not outcome then
    return "not synced yet"
  end
  local kind = outcome.kind
  if kind == "up_to_date" then
    return "up to date"
  elseif kind == "synced" then
    local parts = {}
    if outcome.committed > 0 then
      table.insert(parts, ("saved %d %s"):format(outcome.committed, outcome.committed == 1 and "change" or "changes"))
    end
    if outcome.pulled then
      table.insert(parts, "pulled")
    end
    if outcome.combined > 0 then
      table.insert(parts, ("combined %d %s from two machines"):format(outcome.combined, outcome.combined == 1 and "edit" or "edits"))
    end
    if outcome.pushed then
      table.insert(parts, "pushed")
    end
    return table.concat(parts, ", ")
  elseif kind == "local" then
    return ("saved %d changes (no remote to push to)"):format(outcome.committed)
  elseif kind == "key_locked" then
    return "paused, the key is locked: " .. outcome.message
  elseif kind == "offline" then
    return "offline: " .. outcome.message
  elseif kind == "conflict" then
    return "both machines changed the same lines in " .. table.concat(outcome.files, ", ")
  elseif kind == "busy" then
    return "another sync is running"
  elseif kind == "not_a_repo" then
    return "the vault is not a git repository (run `den init`)"
  end
  return "failed: " .. (outcome.message or "?")
end

--- The statusline segment: quiet unless something needs attention.
function M.segment()
  local s = status
  if not s then
    return nil
  end
  if s.running and announce then
    return { text = "syncing…", hl = "DenSync" }
  end
  local waiting = s.snapshot and ((s.snapshot.changes or 0) + (s.snapshot.ahead or 0)) or 0
  local tail = waiting > 0 and (" · %d waiting"):format(waiting) or ""
  local conflicts = s.snapshot and s.snapshot.conflicts or {}
  local kind = s.outcome and s.outcome.kind
  if #conflicts > 0 or kind == "conflict" or (s.snapshot and s.snapshot.rebasing) then
    return { text = "sync conflict · :Den sync", hl = "DenSyncProblem" }
  elseif kind == "key_locked" then
    return { text = "sync paused · key locked" .. tail, hl = "DenSyncProblem" }
  elseif kind == "offline" then
    return { text = "offline" .. tail, hl = "DenSync" }
  elseif kind == "failed" then
    return { text = "sync failed · :Den sync", hl = "DenSyncProblem" }
  end
  return nil
end

--- Files with conflicts right now.
function M.conflicted()
  local s = status or native.call("sync_status") or {}
  local files = s.snapshot and s.snapshot.conflicts or {}
  if #files == 0 and s.outcome and s.outcome.kind == "conflict" then
    files = s.outcome.files
  end
  return files
end

local function on_events(events)
  local synced = false
  for _, ev in ipairs(events) do
    if ev.kind == "sync" then
      synced = true
    elseif ev.kind == "changed" or ev.kind == "log" then
      M.after_edit()
    end
  end
  if not synced then
    return
  end
  -- A start and its finish can arrive in one batch, so this reads where the
  -- sync is now rather than counting events.
  local last_at = status and status.at
  status = native.call("sync_status")
  pcall(vim.cmd, "redrawstatus")
  if not status or status.running then
    return
  end
  local snap = status.snapshot
  if status.at ~= last_at or (snap and (snap.changes or 0) + (snap.ahead or 0) == 0) then
    edited_since_sync = false
  end
  if announce then
    announce = false
    local outcome = status.outcome
    local bad = outcome and outcome.kind ~= "up_to_date" and outcome.kind ~= "synced" and outcome.kind ~= "local"
    vim.notify("Den: " .. M.describe(outcome), bad and vim.log.levels.WARN or vim.log.levels.INFO)
    if outcome and outcome.kind == "conflict" then
      require("den.screens.conflicts").open()
    end
  end
end

--- `:Den sync [continue]`: opens the conflict screen when there is one,
--- else syncs.
function M.command(args)
  if args[1] == "continue" then
    return M.run({ resume = true })
  end
  if #M.conflicted() > 0 then
    require("den.screens.conflicts").open()
    return true
  end
  return M.run()
end

local listening = false

function M.setup()
  M.stop()
  status = nil
  statusline.register("sync", "status", 90, M.segment)
  if not listening then
    state.on_change(on_events)
    listening = true
  end
  local group = vim.api.nvim_create_augroup("den.sync", { clear = true })
  -- A save that was holding a sync back lets it go.
  vim.api.nvim_create_autocmd("BufWritePost", {
    group = group,
    callback = function()
      if waiting_for_save and not editing() then
        M.after_edit()
      end
    end,
  })
  -- Leaving: hand anything unsent to a `den sync` that outlives Neovim.
  vim.api.nvim_create_autocmd("VimLeavePre", {
    group = group,
    callback = function()
      local s = status
      local repo = s and s.snapshot and s.snapshot.repo
      local waiting = repo and ((s.snapshot.changes or 0) + (s.snapshot.ahead or 0)) or 0
      local program = M.program()
      if enabled() and repo and (waiting > 0 or edited_since_sync) and program and state.info then
        pcall(vim.system, { program, "sync", "--vault", state.info.root }, {
          detach = true,
          stdin = false,
          stdout = false,
          stderr = false,
        })
      end
    end,
  })
  state.when_ready(function()
    if not enabled() then
      return
    end
    status = native.call("sync_status")
    M.background()
    local minutes = settings().every_minutes or 5
    every_timer = vim.uv.new_timer()
    every_timer:start(minutes * 60000, minutes * 60000, vim.schedule_wrap(function()
      M.background()
    end))
  end)
end

--- Stops the timers (tests, and :Den restarts).
function M.stop()
  for _, t in ipairs({ settle_timer, every_timer }) do
    if t and not t:is_closing() then
      t:stop()
      t:close()
    end
  end
  settle_timer, every_timer = nil, nil
end

return M
