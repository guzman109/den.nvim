-- The statusline component: the running timer, then at most two things that
-- need attention, then sync state when something is waiting or wrong.
--
-- Statuslines redraw constantly, so drawing never asks the engine anything.
-- `refresh()` reads the engine when Den has news and caches the answer; while
-- a timer runs, a one-second tick redraws so the clock moves.
--
-- Use it directly:        vim.o.statusline = "%{%v:lua.require'den.statusline'.string()%}"
-- or inside mini.statusline / lualine with `string()` or `segments()`.

local native = require("den.native")
local state = require("den.state")
local util = require("den.util")

local M = {}

local cache = { running = nil, insights = {}, project = nil }
local extras = {}
local tick

--- Adds a source of statusline items. `fn()` returns `{ text, hl }` or nil.
--- `kind` is "insight" (competes for the two insight places, lower
--- `priority` first) or "status" (always shown, after the insights).
function M.register(name, kind, priority, fn)
  extras[name] = { kind = kind, priority = priority, fn = fn }
end

local function ensure_tick()
  if cache.running and not tick then
    tick = vim.uv.new_timer()
    tick:start(1000, 1000, vim.schedule_wrap(function()
      vim.cmd("redrawstatus")
    end))
  elseif not cache.running and tick then
    tick:stop()
    tick:close()
    tick = nil
  end
end

--- Reads the engine and caches what the statusline shows.
function M.refresh()
  if not state.ready or not native.get() then
    return
  end
  local p = native.call("project_for_dir", vim.fn.getcwd())
  cache.project = p and p.name or nil
  cache.insights = native.call("insights", cache.project) or {}
  local r = native.call("timer_running")
  cache.running = r and { task = r.task, file = r.file, started = os.time() - r.seconds } or nil
  ensure_tick()
  pcall(vim.cmd, "redrawstatus")
end

local function short(text, width)
  if util.width(text) <= width then
    return text
  end
  return util.fit(text, width):gsub("%s+$", "")
end

local INSIGHT = {
  overdue = function(i)
    return { text = i.count .. " overdue  " .. short(i.first.title, 24), hl = "DenStatusOverdue" }, 20
  end,
  due_today = function(i)
    return { text = "due today  " .. short(i.first.title, 24), hl = "DenStatusDue" }, 30
  end,
  behind_pace = function(i)
    return { text = ("%d left · %d days · behind pace"):format(i.left, i.days), hl = "DenStatusDue" }, 40
  end,
  inbox = function(i)
    return { text = i.count .. " inbox", hl = "DenStatus" }, 50
  end,
  done_today = function(i)
    return { text = i.count .. " done today", hl = "DenStatusDone" }, 60
  end,
}

--- What to show, as `{ text, hl }` items in order.
function M.segments(max_insights)
  local out = {}
  if cache.running then
    local secs = os.time() - cache.running.started
    table.insert(out, { text = "◐ " .. short(cache.running.task, 28) .. "  " .. util.clock(secs), hl = "DenStatusTimer" })
  end
  local candidates, statuses = {}, {}
  for _, ins in ipairs(cache.insights) do
    local make = INSIGHT[ins.kind]
    if make then
      local seg, priority = make(ins)
      table.insert(candidates, { seg = seg, priority = priority })
    end
  end
  for _, extra in pairs(extras) do
    local ok, seg = pcall(extra.fn)
    if ok and seg then
      if extra.kind == "status" then
        table.insert(statuses, { seg = seg, priority = extra.priority })
      else
        table.insert(candidates, { seg = seg, priority = extra.priority })
      end
    end
  end
  table.sort(candidates, function(a, b)
    return a.priority < b.priority
  end)
  table.sort(statuses, function(a, b)
    return a.priority < b.priority
  end)
  for i = 1, math.min(max_insights or 2, #candidates) do
    table.insert(out, candidates[i].seg)
  end
  for _, s in ipairs(statuses) do
    table.insert(out, s.seg)
  end
  return out
end

--- The component as statusline text, with highlight markers.
function M.string(max_insights)
  local parts = {}
  for _, seg in ipairs(M.segments(max_insights)) do
    table.insert(parts, "%#" .. seg.hl .. "# " .. seg.text:gsub("%%", "%%%%") .. " %*")
  end
  return table.concat(parts, "")
end

--- Plain text, for tests and for statuslines that do their own colours.
function M.text(max_insights)
  local parts = {}
  for _, seg in ipairs(M.segments(max_insights)) do
    table.insert(parts, seg.text)
  end
  return table.concat(parts, " · ")
end

function M.setup()
  state.on_change(M.refresh)
  vim.api.nvim_create_autocmd("DirChanged", {
    group = vim.api.nvim_create_augroup("den.statusline", { clear = true }),
    callback = M.refresh,
  })
  state.when_ready(M.refresh)
end

return M
