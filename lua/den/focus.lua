-- Focus rings: this session, today's focus against the daily goal, and
-- steps (when there is step data).
--
-- `:Den focus` shows them in a corner window sized to the editor (about a
-- third of its height), which never takes the cursor. `:Den focus tab`
-- opens them full size in a tab of their own. In kitty the rings are
-- concentric circles redrawn every second; elsewhere they are meters.

local native = require("den.native")
local util = require("den.util")
local charts = require("den.charts")

local M = {}

local ns = vim.api.nvim_create_namespace("den.focus")
-- The corner window and the tab are separate views; either can be open.
local views = {}

local function fraction(v, goal)
  return goal and goal > 0 and v / goal or 0
end

--- Ring size in cells for a view: a cell is about twice as tall as wide.
local function ring_size(view)
  if view.tab then
    local height = vim.api.nvim_win_get_height(view.win)
    local width = vim.api.nvim_win_get_width(view.win)
    local rows = math.max(8, math.min(height - 4, 40))
    local cols = math.min(rows * 2, width - 40)
    return math.max(cols, 16), math.max(math.floor(cols / 2), 8)
  end
  local rows = math.max(8, math.min(math.floor(vim.o.lines * 0.3), 18))
  return rows * 2, rows
end

local function info(f)
  local steps = f.steps
      and ("%d / %d%s"):format(f.steps, f.steps_goal, f.steps_as_of and os.date(" · %H:%M", util.from_iso(f.steps_as_of)) or "")
    or "no data yet"
  return {
    { f.task and ("◐ " .. f.task) or "no timer running", f.task and "DenTimer" or "DenMuted" },
    { "", "DenMuted" },
    { ("session  %s / %s"):format(util.clock(f.session), util.duration(f.session_goal)), "DenTimer" },
    { ("today    %s / %s"):format(util.duration(f.today), util.duration(f.today_goal)), "DenChart" },
    { "steps    " .. steps, f.steps and "DenDone" or "DenMuted" },
  }
end

--- The view's lines as `{ {text, hl}, … }` for focus data `f`: the rings on
--- the left, the words beside them, vertically centred.
function M.lines(f, cols, rows, meter_width)
  cols, rows = cols or 24, rows or 12
  local words = info(f)
  local kitty = require("den.kitty")
  local rings = kitty.lines("focus-" .. cols .. "x" .. rows, "rings", {
    rings = { fraction(f.session, f.session_goal), fraction(f.today, f.today_goal), f.steps and fraction(f.steps, f.steps_goal) or -1 },
  }, cols, rows)
  local out = {}
  if rings then
    local top = math.max(0, math.floor((#rings - #words) / 2))
    for i, row in ipairs(rings) do
      local w = words[i - top]
      table.insert(out, {
        { row.text, row.group },
        { "   " .. (w and w[1] or ""), w and w[2] or "DenMuted" },
      })
    end
    return out
  end
  -- No images: the words with meters, one ring per line.
  meter_width = meter_width or 24
  local meters = {
    [3] = charts.meter(math.min(f.session, f.session_goal), f.session_goal, meter_width),
    [4] = charts.meter(math.min(f.today, f.today_goal), f.today_goal, meter_width),
    [5] = f.steps and charts.meter(math.min(f.steps, f.steps_goal), f.steps_goal, meter_width) or nil,
  }
  for i, w in ipairs(words) do
    local line = { { " " .. w[1], w[2] } }
    if meters[i] then
      table.insert(line, { "   " .. meters[i], w[2] })
    end
    table.insert(out, line)
  end
  return out
end

local function render(view, f)
  local cols, rows = ring_size(view)
  local lines = M.lines(f, cols, rows, view.tab and 40 or 24)
  local width = 0
  for _, parts in ipairs(lines) do
    local s = 0
    for _, p in ipairs(parts) do
      s = s + util.width(p[1])
    end
    width = math.max(width, s)
  end
  -- In a tab, centred in the window.
  local left, above = "", 0
  if view.tab then
    local ww = vim.api.nvim_win_get_width(view.win)
    local wh = vim.api.nvim_win_get_height(view.win)
    left = string.rep(" ", math.max(0, math.floor((ww - width) / 2)))
    above = math.max(0, math.floor((wh - #lines) / 2))
  end
  local text = {}
  for _ = 1, above do
    table.insert(text, "")
  end
  for _, parts in ipairs(lines) do
    local s = left
    for _, p in ipairs(parts) do
      s = s .. p[1]
    end
    table.insert(text, s)
  end
  vim.bo[view.buf].modifiable = true
  vim.api.nvim_buf_set_lines(view.buf, 0, -1, false, text)
  vim.bo[view.buf].modifiable = false
  vim.bo[view.buf].modified = false
  vim.api.nvim_buf_clear_namespace(view.buf, ns, 0, -1)
  for i, parts in ipairs(lines) do
    local col = #left
    for _, p in ipairs(parts) do
      if #p[1] > 0 then
        vim.api.nvim_buf_set_extmark(view.buf, ns, above + i - 1, col, { end_col = col + #p[1], hl_group = p[2] })
      end
      col = col + #p[1]
    end
  end
  if not view.tab then
    vim.api.nvim_win_set_config(view.win, {
      relative = "editor",
      anchor = "NE",
      row = 1,
      col = vim.o.columns - 1,
      width = math.min(width + 2, vim.o.columns - 4),
      height = math.min(#text, vim.o.lines - 4),
    })
  end
end

local function close(key)
  local view = views[key]
  if not view then
    return
  end
  views[key] = nil
  if view.timer and not view.timer:is_closing() then
    view.timer:stop()
    view.timer:close()
  end
  if view.tab then
    if view.buf and vim.api.nvim_buf_is_valid(view.buf) then
      pcall(vim.api.nvim_buf_delete, view.buf, { force = true })
    end
  elseif view.win and vim.api.nvim_win_is_valid(view.win) then
    vim.api.nvim_win_close(view.win, true)
  end
end

local function draw(key)
  local view = views[key]
  if not view or not vim.api.nvim_win_is_valid(view.win) or not vim.api.nvim_buf_is_valid(view.buf) then
    close(key)
    return
  end
  local f = native.call("focus")
  if f then
    render(view, f)
  end
end

local function start(key, view)
  views[key] = view
  draw(key)
  view.timer = vim.uv.new_timer()
  view.timer:start(1000, 1000, vim.schedule_wrap(function()
    draw(key)
  end))
end

--- The corner window.
function M.open()
  if views.corner then
    return
  end
  local buf = vim.api.nvim_create_buf(false, true)
  vim.bo[buf].bufhidden = "wipe"
  local win = vim.api.nvim_open_win(buf, false, {
    relative = "editor",
    anchor = "NE",
    row = 1,
    col = vim.o.columns - 1,
    width = 40,
    height = 8,
    style = "minimal",
    border = "rounded",
    title = " focus ",
    title_pos = "center",
    focusable = false,
    noautocmd = true,
  })
  start("corner", { buf = buf, win = win, tab = false })
end

--- A tab of its own, full size. `q` closes it.
function M.open_tab()
  if views.tab and vim.api.nvim_win_is_valid(views.tab.win) then
    vim.api.nvim_set_current_win(views.tab.win)
    return
  end
  vim.cmd("tabnew")
  local win = vim.api.nvim_get_current_win()
  local buf = vim.api.nvim_get_current_buf()
  vim.bo[buf].buftype = "nofile"
  vim.bo[buf].bufhidden = "wipe"
  vim.bo[buf].swapfile = false
  pcall(vim.api.nvim_buf_set_name, buf, "den://focus")
  for option, value in pairs({ number = false, relativenumber = false, signcolumn = "no", cursorline = false, list = false, wrap = false }) do
    vim.wo[win][option] = value
  end
  vim.keymap.set("n", "q", function()
    close("tab")
    pcall(vim.cmd, "tabclose")
  end, { buffer = buf, nowait = true, desc = "close the focus tab" })
  start("tab", { buf = buf, win = win, tab = true })
end

function M.close()
  close("corner")
end

function M.toggle()
  if views.corner then
    close("corner")
  else
    M.open()
  end
end

function M.is_open()
  return views.corner ~= nil and vim.api.nvim_win_is_valid(views.corner.win)
end

function M.tab_open()
  return views.tab ~= nil and vim.api.nvim_win_is_valid(views.tab.win)
end

--- Redraws open views at their new size.
function M.resized()
  for key in pairs(views) do
    draw(key)
  end
end

return M
