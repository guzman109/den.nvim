-- Focus rings: a small corner window with this session, today's focus
-- against the daily goal, and steps (when there is step data).
--
-- In kitty the rings are drawn as concentric circles and redrawn every
-- second; elsewhere they are meters. `:Den focus` opens and closes it; it
-- never takes the cursor.

local native = require("den.native")
local util = require("den.util")
local charts = require("den.charts")

local M = {}

local ns = vim.api.nvim_create_namespace("den.focus")
local win, buf, timer

local RING_COLS, RING_ROWS = 12, 6

local function fraction(v, goal)
  return goal and goal > 0 and v / goal or 0
end

--- The window's lines and highlights for focus data `f`.
function M.lines(f)
  local info = {
    { f.task and ("◐ " .. f.task) or "no timer running", f.task and "DenTimer" or "DenMuted" },
    { ("session  %s / %s"):format(util.clock(f.session), util.duration(f.session_goal)), "DenTimer" },
    { ("today    %s / %s"):format(util.duration(f.today), util.duration(f.today_goal)), "DenChart" },
    {
      f.steps and ("steps    %d / %d%s"):format(
        f.steps,
        f.steps_goal,
        f.steps_as_of and os.date(" · %H:%M", util.from_iso(f.steps_as_of)) or ""
      ) or "steps    no data yet",
      f.steps and "DenDone" or "DenMuted",
    },
  }
  local kitty = require("den.kitty")
  local rings = kitty.lines("focus", "rings", {
    rings = { fraction(f.session, f.session_goal), fraction(f.today, f.today_goal), f.steps and fraction(f.steps, f.steps_goal) or -1 },
  }, RING_COLS, RING_ROWS)
  local out = {}
  if rings then
    for i, row in ipairs(rings) do
      local text = info[i - 1]
      table.insert(out, {
        { row.text, row.group },
        { "  " .. (text and text[1] or ""), text and text[2] or "DenMuted" },
      })
    end
  else
    local meters = {
      nil,
      charts.meter(math.min(f.session, f.session_goal), f.session_goal, 10),
      charts.meter(math.min(f.today, f.today_goal), f.today_goal, 10),
      f.steps and charts.meter(math.min(f.steps, f.steps_goal), f.steps_goal, 10) or nil,
    }
    for i, text in ipairs(info) do
      local line = { { " " .. text[1], text[2] } }
      if meters[i] then
        table.insert(line, { "  " .. meters[i], text[2] })
      end
      table.insert(out, line)
    end
  end
  return out
end

local function draw()
  if not win or not vim.api.nvim_win_is_valid(win) then
    M.close()
    return
  end
  local f = native.call("focus")
  if not f then
    return
  end
  local lines = M.lines(f)
  local text, width = {}, 0
  for i, parts in ipairs(lines) do
    local s = ""
    for _, p in ipairs(parts) do
      s = s .. p[1]
    end
    text[i] = s
    width = math.max(width, util.width(s) + 1)
  end
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, text)
  vim.bo[buf].modifiable = false
  vim.api.nvim_buf_clear_namespace(buf, ns, 0, -1)
  for i, parts in ipairs(lines) do
    local col = 0
    for _, p in ipairs(parts) do
      if #p[1] > 0 then
        vim.api.nvim_buf_set_extmark(buf, ns, i - 1, col, { end_col = col + #p[1], hl_group = p[2] })
      end
      col = col + #p[1]
    end
  end
  vim.api.nvim_win_set_config(win, {
    relative = "editor",
    anchor = "NE",
    row = 1,
    col = vim.o.columns - 1,
    width = math.min(width, vim.o.columns - 4),
    height = #text,
  })
end

function M.open()
  if win and vim.api.nvim_win_is_valid(win) then
    return
  end
  buf = vim.api.nvim_create_buf(false, true)
  vim.bo[buf].bufhidden = "wipe"
  win = vim.api.nvim_open_win(buf, false, {
    relative = "editor",
    anchor = "NE",
    row = 1,
    col = vim.o.columns - 1,
    width = 30,
    height = 4,
    style = "minimal",
    border = "rounded",
    title = " focus ",
    title_pos = "center",
    focusable = false,
    noautocmd = true,
  })
  draw()
  timer = vim.uv.new_timer()
  timer:start(1000, 1000, vim.schedule_wrap(draw))
end

function M.close()
  if timer and not timer:is_closing() then
    timer:stop()
    timer:close()
  end
  timer = nil
  if win and vim.api.nvim_win_is_valid(win) then
    vim.api.nvim_win_close(win, true)
  end
  win = nil
end

function M.toggle()
  if win and vim.api.nvim_win_is_valid(win) then
    M.close()
  else
    M.open()
  end
end

function M.is_open()
  return win ~= nil and vim.api.nvim_win_is_valid(win)
end

return M
