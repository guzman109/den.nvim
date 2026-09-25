-- Drawing task rows in columns, shared by the Tasks and Inbox screens.

local S = require("den.ui.screen")
local util = require("den.util")

local M = {}

M.icons = {
  open = { "○", "DenOpen" },
  doing = { "◐", "DenDoing" },
  dropped = { "⊘", "DenDropped" },
  done = { "✓", "DenDone" },
}

--- Column widths for a window `width` cells wide, sized to the rows shown.
--- The project column is only filled for rows that name their project; other
--- rows let their title run into it.
function M.columns(width, list, with_project)
  local tags, project = 0, 0
  for _, row in ipairs(list or {}) do
    tags = math.max(tags, util.width(M.tags_text(row.tags)))
    if with_project and row.project then
      project = math.max(project, util.width(row.project.title))
    end
  end
  local cols = {
    indent = 4,
    icon = 3,
    tags = tags > 0 and math.min(tags, 24) + 2 or 0,
    right = 12,
    project = project > 0 and math.min(project, 24) + 2 or 0,
  }
  cols.title = math.max(16, width - cols.indent - cols.icon - cols.project - cols.tags - cols.right - 1)
  return cols
end

function M.tags_text(tags)
  local parts = {}
  for _, t in ipairs(tags or {}) do
    table.insert(parts, "#" .. t)
  end
  return table.concat(parts, " ")
end

--- The right-hand column: the running clock, an age, a due date, or when it
--- was finished.
local function right(row, ctx)
  if ctx.running and ctx.running.file == row.path and ctx.running.task == row.title then
    return util.clock(ctx.running.seconds), "DenTimer"
  end
  if row.state == "done" and row.done then
    return util.short_date(row.done), "DenMuted"
  end
  if row.due and row.state ~= "dropped" then
    local n = util.days_between(ctx.today, row.due) or 1
    local hl = n < 0 and "DenOverdue" or (n == 0 and "DenDueToday" or "DenDue")
    return util.relative(row.due, ctx.today), hl
  end
  if row.age then
    return row.age, "DenMuted"
  end
  return "", nil
end

--- One task as a screen line.
function M.line(row, cols, ctx, show_project)
  local l = S.line()
  S.add(l, string.rep(" ", cols.indent))
  local icon = M.icons[row.state] or M.icons.open
  S.cell(l, icon[1], cols.icon, icon[2])
  local title_hl = (row.state == "done" or row.state == "dropped") and "DenMuted" or nil
  if cols.project > 0 and show_project then
    S.cell(l, row.title, cols.title, title_hl)
    S.cell(l, row.project and row.project.title or "", cols.project, "DenProject")
  else
    S.cell(l, row.title, cols.title + cols.project, title_hl)
  end
  if cols.tags > 0 then
    S.cell(l, M.tags_text(row.tags), cols.tags, "DenTag")
  end
  local text, hl = right(row, ctx)
  S.add(l, text, hl)
  l.text = l.text:gsub("%s+$", "")
  return l
end

--- Whether a row matches a filter (case-insensitive, in title, tags or
--- project).
function M.matches(row, filter)
  if not filter or filter == "" then
    return true
  end
  local hay = (row.title .. " " .. M.tags_text(row.tags) .. " " .. (row.project and row.project.title or "")):lower()
  return hay:find(filter:lower(), 1, true) ~= nil
end

--- A header line with a title on the left and counts on the right.
function M.header(title, counts, width)
  local l = S.line()
  S.add(l, "  ")
  S.add(l, title, "DenHeader")
  local gap = width - 2 - util.width(title) - util.width(counts) - 2
  S.add(l, string.rep(" ", math.max(2, gap)))
  S.add(l, counts, "DenMuted")
  return l
end

--- The key hints at the bottom of a screen: { {key, words}, ... }.
function M.hints(pairs_)
  local l = S.line()
  S.add(l, "  ")
  for i, pair in ipairs(pairs_) do
    if i > 1 then
      S.add(l, "   ")
    end
    S.add(l, pair[1], "DenKey")
    S.add(l, " " .. pair[2], "DenMuted")
  end
  return l
end

--- A stable identity for keeping the cursor on the same task after a redraw.
function M.key(row)
  return row.path .. "\0" .. row.title
end

return M
