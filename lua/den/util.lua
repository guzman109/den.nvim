-- Small helpers: dates, widths, durations.

local M = {}

local MONTHS = { "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec" }

--- "2026-09-28" → os.time at noon that day, or nil.
function M.parse_date(s)
  local y, m, d = tostring(s or ""):match("^(%d%d%d%d)%-(%d%d)%-(%d%d)$")
  if not y then
    return nil
  end
  return os.time({ year = tonumber(y), month = tonumber(m), day = tonumber(d), hour = 12 })
end

--- Whole days from `a` to `b` (dates as strings).
function M.days_between(a, b)
  local ta, tb = M.parse_date(a), M.parse_date(b)
  if not ta or not tb then
    return nil
  end
  return math.floor((tb - ta) / 86400 + 0.5)
end

--- "2026-09-28" → "28 Sep".
function M.short_date(s)
  local _, m, d = tostring(s or ""):match("^(%d%d%d%d)%-(%d%d)%-(%d%d)$")
  if not m then
    return s
  end
  return tonumber(d) .. " " .. MONTHS[tonumber(m)]
end

--- How far a date is from today, in words: "today", "in 4 days", "2 days ago".
function M.relative(date, today)
  local n = M.days_between(today, date)
  if not n then
    return date
  end
  if n == 0 then
    return "today"
  elseif n == 1 then
    return "tomorrow"
  elseif n == -1 then
    return "yesterday"
  elseif n > 1 and n <= 7 then
    return "in " .. n .. " days"
  elseif n < -1 and n >= -7 then
    return (-n) .. " days ago"
  end
  return M.short_date(date)
end

function M.width(s)
  return vim.fn.strdisplaywidth(s)
end

--- Pads or truncates `s` to exactly `w` display cells.
function M.fit(s, w)
  s = s or ""
  if w <= 0 then
    return ""
  end
  local width = M.width(s)
  if width <= w then
    return s .. string.rep(" ", w - width)
  end
  local out, used = {}, 0
  for _, ch in ipairs(vim.fn.split(s, "\\zs")) do
    local cw = M.width(ch)
    if used + cw > w - 1 then
      break
    end
    table.insert(out, ch)
    used = used + cw
  end
  return table.concat(out) .. "…" .. string.rep(" ", w - used - 1)
end

--- 4320 → "1h 12m"; 300 → "5m"; 20 → "20s".
function M.duration(seconds)
  seconds = math.max(0, math.floor(seconds or 0))
  local h, m = math.floor(seconds / 3600), math.floor(seconds % 3600 / 60)
  if h > 0 then
    return m > 0 and (h .. "h " .. m .. "m") or (h .. "h")
  elseif m > 0 then
    return m .. "m"
  end
  return seconds .. "s"
end

--- 1450 → "24:10"; 4324 → "1:12:04".
function M.clock(seconds)
  seconds = math.max(0, math.floor(seconds or 0))
  local h, m, s = math.floor(seconds / 3600), math.floor(seconds % 3600 / 60), seconds % 60
  if h > 0 then
    return string.format("%d:%02d:%02d", h, m, s)
  end
  return string.format("%02d:%02d", m, s)
end

--- How long ago something was written: "12m", "5h", "3d", "2w", "4mo".
function M.age(seconds)
  seconds = math.max(0, math.floor(seconds or 0))
  if seconds < 3600 then
    return math.max(1, math.floor(seconds / 60)) .. "m"
  elseif seconds < 86400 then
    return math.floor(seconds / 3600) .. "h"
  elseif seconds < 14 * 86400 then
    return math.floor(seconds / 86400) .. "d"
  elseif seconds < 60 * 86400 then
    return math.floor(seconds / (7 * 86400)) .. "w"
  end
  return math.floor(seconds / (30 * 86400)) .. "mo"
end

--- "2026-09-24T23:25:39Z" (UTC) → os.time value, or nil.
function M.from_iso(s)
  local y, mo, d, h, mi, se = tostring(s or ""):match("^(%d+)%-(%d+)%-(%d+)T(%d+):(%d+):(%d+)")
  if not y then
    return nil
  end
  local t = os.time({ year = tonumber(y), month = tonumber(mo), day = tonumber(d), hour = tonumber(h), min = tonumber(mi), sec = tonumber(se), isdst = false })
  -- os.time read that as local time; shift by the zone's offset at that moment.
  local offset = os.time(os.date("*t", t)) - os.time(os.date("!*t", t))
  return t + offset
end

--- A task row's reference for the engine.
function M.ref(row)
  return { path = row.path, line = row.line, raw = row.raw }
end

return M
