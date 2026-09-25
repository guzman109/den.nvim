-- The session's link to the engine: setup, the wake-up pipe, and change
-- events for whoever wants to redraw.

local native = require("den.native")

local M = {
  ready = false,
  info = nil,
  listeners = {},
  waiting = {},
  pipe = nil,
}

local function drain()
  local mod = native.get()
  if not mod then
    return
  end
  local ok, events = pcall(mod.poll)
  if not ok or not events or #events == 0 then
    return
  end
  local became_ready = false
  for _, ev in ipairs(events) do
    if ev.kind == "loaded" then
      M.ready = true
      became_ready = true
    elseif ev.kind == "error" then
      vim.notify("Den: " .. ev.message, vim.log.levels.ERROR)
    end
  end
  if became_ready then
    local waiting = M.waiting
    M.waiting = {}
    for _, fn in ipairs(waiting) do
      local fine, err = pcall(fn)
      if not fine then
        vim.notify("Den: " .. tostring(err), vim.log.levels.ERROR)
      end
    end
  end
  for _, fn in ipairs(M.listeners) do
    pcall(fn, events)
  end
end

--- Starts the engine: loads config, then the vault on a background thread.
function M.setup(opts)
  local mod = native.need()
  local info = mod.setup(opts or {})
  M.info = info
  M.ready = false
  if M.pipe and not M.pipe:is_closing() then
    M.pipe:close()
  end
  local pipe = vim.uv.new_pipe(false)
  pipe:open(info.fd)
  pipe:read_start(function(err, data)
    if err or not data then
      if not pipe:is_closing() then
        pipe:close()
      end
      return
    end
    vim.schedule(drain)
  end)
  M.pipe = pipe
  return info
end

--- Runs `fn` now if the vault is loaded, else as soon as it is.
function M.when_ready(fn)
  if M.ready then
    fn()
  else
    table.insert(M.waiting, fn)
  end
end

--- Calls `fn(events)` whenever the engine has news (a load, outside changes).
function M.on_change(fn)
  table.insert(M.listeners, fn)
end

--- Waits (processing events) until the vault is loaded or `ms` pass.
function M.wait(ms)
  return vim.wait(ms or 5000, function()
    return M.ready
  end, 10)
end

--- Tells listeners the vault changed because of something Den did.
function M.changed(paths)
  for _, fn in ipairs(M.listeners) do
    pcall(fn, { { kind = "changed", paths = paths or {} } })
  end
end

return M
