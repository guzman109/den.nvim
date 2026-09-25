-- Den's own screens: read-only `den://` buffers drawn from engine data.
--
-- A screen supplies `render(ctx, width)` returning lines built with `line()`
-- and `add()`, plus the item each line stands for. The buffer is redrawn
-- whenever the vault changes while the screen is visible, and the cursor
-- stays on the same item when it can.

local util = require("den.util")

local M = {}

local ns = vim.api.nvim_create_namespace("den.screen")
local screens = {}

--- A line under construction.
function M.line()
  return { text = "", marks = {} }
end

--- Appends `text` to a line, highlighted with `hl` when given.
function M.add(line, text, hl)
  local start = #line.text
  line.text = line.text .. text
  if hl and #text > 0 then
    table.insert(line.marks, { start, #line.text, hl })
  end
  return line
end

--- Appends `text` padded or cut to `width` cells.
function M.cell(line, text, width, hl)
  return M.add(line, util.fit(text, width), hl)
end

local function set_options(win)
  local wo = vim.wo[win]
  wo.number = false
  wo.relativenumber = false
  wo.signcolumn = "no"
  wo.foldcolumn = "0"
  wo.cursorline = true
  wo.wrap = false
  wo.list = false
  wo.spell = false
  wo.conceallevel = 0
end

local function buffer(name)
  local bufname = "den://" .. name
  local buf = vim.fn.bufnr(bufname)
  if buf ~= -1 and vim.api.nvim_buf_is_valid(buf) then
    return buf
  end
  buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_name(buf, bufname)
  vim.bo[buf].buftype = "nofile"
  vim.bo[buf].bufhidden = "hide"
  vim.bo[buf].swapfile = false
  vim.bo[buf].modifiable = false
  vim.bo[buf].filetype = "den"
  return buf
end

--- Opens (or focuses) a screen in the current window.
---@param name string
---@param spec table { render = fn(ctx, width) -> {lines, items}, keys = { [lhs] = { fn(item, ctx), desc } }, ctx = table }
function M.open(name, spec)
  local s = screens[name] or {}
  s.spec = spec
  s.ctx = vim.tbl_extend("keep", s.ctx or {}, spec.ctx or {})
  s.buf = buffer(name)
  screens[name] = s
  vim.api.nvim_set_current_buf(s.buf)
  set_options(vim.api.nvim_get_current_win())
  for lhs, action in pairs(spec.keys or {}) do
    vim.keymap.set("n", lhs, function()
      local item = M.item(name)
      action[1](item, s.ctx, name)
    end, { buffer = s.buf, nowait = true, desc = action[2] })
  end
  M.render(name)
  return s
end

--- The item under the cursor in a screen, if any.
function M.item(name)
  local s = screens[name]
  if not s or not s.items then
    return nil
  end
  local row = vim.api.nvim_win_get_cursor(0)[1]
  return s.items[row]
end

--- A screen's context table, for its actions to change.
function M.ctx(name)
  return screens[name] and screens[name].ctx
end

local function visible_win(buf)
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    if vim.api.nvim_win_get_buf(win) == buf then
      return win
    end
  end
  return nil
end

--- Redraws a screen if it is showing somewhere.
function M.render(name)
  local s = screens[name]
  if not s or not s.buf or not vim.api.nvim_buf_is_valid(s.buf) then
    return
  end
  local win = visible_win(s.buf)
  if not win then
    return
  end
  local width = vim.api.nvim_win_get_width(win)
  local ok, result = pcall(s.spec.render, s.ctx, width)
  if not ok then
    result = { lines = { M.add(M.line(), "  Den: " .. tostring(result), "ErrorMsg") }, items = {} }
  end

  local cursor_item = s.items and s.items[vim.api.nvim_win_get_cursor(win)[1]]
  local text = {}
  for i, l in ipairs(result.lines) do
    text[i] = l.text
  end
  vim.bo[s.buf].modifiable = true
  vim.api.nvim_buf_set_lines(s.buf, 0, -1, false, text)
  vim.bo[s.buf].modifiable = false
  vim.bo[s.buf].modified = false
  vim.api.nvim_buf_clear_namespace(s.buf, ns, 0, -1)
  for i, l in ipairs(result.lines) do
    for _, m in ipairs(l.marks) do
      local last = #l.text
      if m[1] < last then
        vim.api.nvim_buf_set_extmark(s.buf, ns, i - 1, m[1], { end_col = math.min(m[2], last), hl_group = m[3] })
      end
    end
  end
  s.items = result.items or {}

  local target
  if cursor_item and cursor_item.key then
    for row, item in pairs(s.items) do
      if item.key == cursor_item.key then
        target = row
        break
      end
    end
  end
  if not target then
    local current = vim.api.nvim_win_get_cursor(win)[1]
    if s.items[current] then
      target = current
    else
      local first
      for row in pairs(s.items) do
        if s.items[row].key and (not first or row < first) then
          first = row
        end
      end
      target = math.min(current, #text)
      if first and not s.items[target] then
        target = first
      end
    end
  end
  pcall(vim.api.nvim_win_set_cursor, win, { math.max(1, math.min(target, #text)), 0 })
end

--- Redraws every visible screen.
function M.render_all()
  for name in pairs(screens) do
    M.render(name)
  end
end

--- A small read-only float listing a screen's keys.
function M.help(spec)
  local lines = {}
  local keys = vim.tbl_keys(spec.keys or {})
  table.sort(keys)
  for _, lhs in ipairs(keys) do
    table.insert(lines, string.format("  %-8s %s", lhs, spec.keys[lhs][2] or ""))
  end
  local buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false
  local width = 0
  for _, l in ipairs(lines) do
    width = math.max(width, util.width(l) + 2)
  end
  local win = vim.api.nvim_open_win(buf, true, {
    relative = "editor",
    width = width,
    height = #lines,
    row = math.floor((vim.o.lines - #lines) / 2),
    col = math.floor((vim.o.columns - width) / 2),
    style = "minimal",
    border = "rounded",
    title = " keys ",
    title_pos = "center",
  })
  for _, lhs in ipairs({ "q", "<Esc>", "g?" }) do
    vim.keymap.set("n", lhs, function()
      pcall(vim.api.nvim_win_close, win, true)
    end, { buffer = buf, nowait = true })
  end
end

return M
