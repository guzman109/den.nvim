-- The daily journal page: created from templates/daily.md the first time it
-- is opened each day. The day's facts are drawn under its title (see
-- decor.lua). Late in the day, if nothing has been written yet, the
-- statusline says so once, quietly. No streaks.

local native = require("den.native")
local apply = require("den.apply")
local state = require("den.state")

local M = {}

--- From this hour on, an unwritten page is mentioned in the statusline.
M.LATE_HOUR = 20

local written = nil

--- Whether a page has anything in it beyond the template's headings.
function M.has_writing(text)
  local body = text:gsub("^%-%-%-\n.-\n%-%-%-\n", "", 1)
  for line in body:gmatch("[^\n]+") do
    if not line:match("^%s*$") and not line:match("^#") then
      return true
    end
  end
  return false
end

--- Re-reads whether today's page (or `date`'s) has been written.
function M.refresh(date)
  if not state.ready then
    return
  end
  local day = type(date) == "string" and date or native.call("today")
  local path = native.call("abs", "daily/" .. day .. ".md")
  local f = path and io.open(path, "r")
  if not f then
    written = false
    return
  end
  written = M.has_writing(f:read("*a"))
  f:close()
end

local function segment()
  if written ~= false or tonumber(os.date("%H")) < M.LATE_HOUR then
    return nil
  end
  return { text = "journal · not written today", hl = "DenMuted" }
end

function M.setup()
  require("den.statusline").register("journal", "insight", 70, segment)
  state.on_change(M.refresh)
  state.when_ready(M.refresh)
end

--- Opens (creating if needed) the page for `date` (default today).
function M.open(date)
  local ok = apply.run(function(mod)
    return mod.plan_daily(date)
  end)
  if not ok then
    return
  end
  local day = date or native.call("today")
  local rel = "daily/" .. day .. ".md"
  vim.cmd.edit(vim.fn.fnameescape(native.call("abs", rel)))
end

return M
