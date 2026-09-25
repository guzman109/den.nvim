-- What Den adds to open vault files. It never changes their text; it only
-- highlights and adds dimmed virtual text:
--
--   - `@due(...)` in its colour, with "in 4 days" after it on open tasks
--   - `@done(...)` dimmed
--   - under a project's title, the notes that belong to it
--
-- markview (or any renderer) draws everything else.

local native = require("den.native")
local util = require("den.util")

local M = {}

local ns = vim.api.nvim_create_namespace("den.decor")
-- Above syntax and Markdown renderers, so a date's colour shows through.
local PRIORITY = 5000
local pending = {}

--- The vault path of a buffer, or nil.
function M.rel(buf)
  if not vim.api.nvim_buf_is_valid(buf) or vim.bo[buf].buftype ~= "" then
    return nil
  end
  local name = vim.api.nvim_buf_get_name(buf)
  if name == "" or not name:match("%.md$") then
    return nil
  end
  local rel = native.call("rel", name)
  return rel
end

local function body_start(lines)
  if lines[1] ~= "---" then
    return 1
  end
  for i = 2, #lines do
    if lines[i] == "---" or lines[i] == "..." then
      return i + 1
    end
  end
  return 1
end

function M.decorate(buf)
  local rel = M.rel(buf)
  if not rel then
    return
  end
  vim.api.nvim_buf_clear_namespace(buf, ns, 0, -1)
  local today = native.call("today")
  local lines = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  local start = body_start(lines)
  local fence
  local title_line
  for i = start, #lines do
    local line = lines[i]
    local marker = line:match("^%s*(```+)") or line:match("^%s*(~~~+)")
    if marker then
      if not fence then
        fence = marker
      elseif marker:sub(1, 1) == fence:sub(1, 1) and #marker >= #fence then
        fence = nil
      end
    elseif not fence then
      if not title_line and line:match("^# ") then
        title_line = i
      end
      local mark = line:match("^%s*[-*+] %[(.)%]")
      local closed = mark == "x" or mark == "X" or mark == "-"
      for s, date, e in line:gmatch("()@due%((%d%d%d%d%-%d%d%-%d%d)%)()") do
        local n = util.days_between(today, date)
        local hl = closed and "DenMuted" or (n and n < 0 and "DenOverdue") or (n == 0 and "DenDueToday") or "DenDue"
        vim.api.nvim_buf_set_extmark(buf, ns, i - 1, s - 1, { end_col = e - 1, hl_group = hl, priority = PRIORITY })
        if mark and not closed then
          local words_hl = (hl == "DenOverdue" or hl == "DenDueToday") and hl or "DenMuted"
          vim.api.nvim_buf_set_extmark(buf, ns, i - 1, 0, {
            virt_text = { { "  " .. util.relative(date, today), words_hl } },
            virt_text_pos = "eol",
          })
        end
      end
      for s, e in line:gmatch("()@done%(%d%d%d%d%-%d%d%-%d%d%)()") do
        vim.api.nvim_buf_set_extmark(buf, ns, i - 1, s - 1, { end_col = e - 1, hl_group = "DenMuted", priority = PRIORITY })
      end
    end
  end

  local name = rel:match("^projects/([^/]+)%.md$")
  if name and title_line then
    local p = native.call("project", name)
    if p and #p.notes > 0 then
      local titles = {}
      for _, n in ipairs(p.notes) do
        table.insert(titles, n.title)
      end
      local count = #p.notes == 1 and "1 note   " or (#p.notes .. " notes   ")
      vim.api.nvim_buf_set_extmark(buf, ns, title_line - 1, 0, {
        virt_lines = { { { "  " .. count, "DenMuted" }, { table.concat(titles, " · "), "DenNote" } } },
      })
    end
  end
end

--- Redraws soon, once, however many edits arrive meanwhile.
function M.schedule(buf)
  if pending[buf] then
    return
  end
  pending[buf] = true
  vim.defer_fn(function()
    pending[buf] = nil
    if vim.api.nvim_buf_is_valid(buf) then
      pcall(M.decorate, buf)
    end
  end, 120)
end

--- Redraws every visible vault buffer.
function M.refresh()
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    M.schedule(vim.api.nvim_win_get_buf(win))
  end
end

return M
