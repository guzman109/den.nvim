-- Charts drawn with block characters, for any terminal.
--
-- Every function returns plain strings; the caller places and colours them.
-- Heights and widths are in cells, and values are scaled in eighths of a
-- cell, so small differences still show.

local M = {}

local H = { "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█" }
local V = { "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█" }

--- A horizontal bar `value / max` of `width` cells.
function M.hbar(value, max, width)
  if not max or max <= 0 or not value or value <= 0 then
    return ""
  end
  local eighths = math.floor(math.min(value / max, 1) * width * 8 + 0.5)
  eighths = math.max(eighths, 1)
  local full, part = math.floor(eighths / 8), eighths % 8
  return string.rep("█", full) .. (part > 0 and H[part] or "")
end

--- A filled and empty meter: `▰▰▰▱▱`.
function M.meter(done, total, width)
  if not total or total <= 0 then
    return string.rep("▱", width)
  end
  local filled = math.floor(done / total * width + 0.5)
  return string.rep("▰", filled) .. string.rep("▱", width - filled)
end

--- Vertical bars, one cell wide each, `height` rows tall. Returns the rows
--- from top to bottom. A zero is drawn as a faint baseline so the days
--- still read as a row.
function M.columns(values, height, max)
  max = max or 0
  for _, v in ipairs(values) do
    max = math.max(max, v)
  end
  local rows = {}
  for r = height, 1, -1 do
    local line = {}
    for _, v in ipairs(values) do
      local eighths = max > 0 and math.floor(v / max * height * 8 + 0.5) or 0
      if v > 0 then
        eighths = math.max(eighths, 1)
      end
      local below = (r - 1) * 8
      local here = eighths - below
      if here >= 8 then
        table.insert(line, "█")
      elseif here > 0 then
        table.insert(line, V[here])
      elseif r == 1 then
        table.insert(line, "·")
      else
        table.insert(line, " ")
      end
    end
    table.insert(rows, table.concat(line))
  end
  return rows
end

--- A burndown: tasks left per day as bars, the straight line to zero at the
--- end date as dots, and room for the days still to come.
---@param points { date: string, count: integer }[] from the start to today
---@param days integer days from the start to the end date, inclusive
---@param height integer rows
---@param width integer cells for the plot
---@return string[] rows top to bottom, then an axis row
function M.burndown(points, days, height, width)
  local total = points[1] and points[1].count or 0
  local max = total
  for _, p in ipairs(points) do
    max = math.max(max, p.count)
  end
  local step = math.max(1, math.floor(width / math.max(days, 1)))
  local cols = math.min(days, math.floor(width / step))
  -- Days per column when the range is wider than the plot.
  local per = days / cols
  local values, ideal = {}, {}
  for c = 1, cols do
    local day = math.floor((c - 1) * per) + 1
    local p = points[day]
    values[c] = p and p.count or nil
    ideal[c] = total * (1 - (day - 1) / math.max(days - 1, 1))
  end
  local rows = {}
  for r = height, 1, -1 do
    local line = {}
    for c = 1, cols do
      local cell = " "
      local v = values[c]
      if v then
        local eighths = max > 0 and math.floor(v / max * height * 8 + 0.5) or 0
        local here = eighths - (r - 1) * 8
        if here >= 8 then
          cell = "█"
        elseif here > 0 then
          cell = V[here]
        end
      end
      -- The ideal line shows as dots, cutting through the bars where the
      -- project is behind it.
      local want = max > 0 and ideal[c] / max * height or 0
      if want > r - 1 and want <= r and (cell == " " or cell == "█") then
        cell = cell == "█" and "╍" or "·"
      end
      table.insert(line, string.rep(cell, step))
    end
    table.insert(rows, table.concat(line))
  end
  table.insert(rows, string.rep("─", cols * step))
  return rows
end

return M
