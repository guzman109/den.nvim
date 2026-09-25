-- The daily journal page: created from templates/daily.md the first time it
-- is opened each day.

local native = require("den.native")
local apply = require("den.apply")

local M = {}

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
