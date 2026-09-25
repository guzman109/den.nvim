-- Images in kitty, placed with Unicode placeholders.
--
-- The engine draws a chart as a PNG. Den sends it to kitty once with an id,
-- then writes rows of a special placeholder character into the buffer,
-- coloured with the image id; kitty paints the image over those cells. The
-- image is ordinary buffer text as far as Neovim is concerned, so it
-- scrolls, folds and redraws like any line.
--
-- Used only in kitty, outside tmux, with 'termguicolors' on and a UI
-- attached; everything else gets block-character charts. Turn it off with
-- `require("den").setup({ images = false })`.

local native = require("den.native")

local M = {}

local PLACEHOLDER = vim.fn.nr2char(0x10EEEE)
-- kitty's row/column diacritics, in order; one per row is all Den needs.
local DIACRITICS = {
  0x0305, 0x030D, 0x030E, 0x0310, 0x0312, 0x033D, 0x033E, 0x033F,
  0x0346, 0x034A, 0x034B, 0x034C, 0x0350, 0x0351, 0x0352, 0x0357,
  0x035B, 0x0363, 0x0364, 0x0365, 0x0366, 0x0367, 0x0368, 0x0369,
}

-- Stable ids per chart name, below 2^24 so they fit in a colour.
local ids = {}
local sent = {}
local next_id = 0xDE0001

local function id_for(name)
  if not ids[name] then
    ids[name] = next_id
    next_id = next_id + 1
  end
  return ids[name]
end

--- Whether to draw images at all.
function M.enabled()
  local opts = package.loaded["den"] and package.loaded["den"].options or {}
  if opts.images == false then
    return false
  end
  if not (vim.env.KITTY_WINDOW_ID or vim.env.TERM == "xterm-kitty") or vim.env.TMUX then
    return false
  end
  return vim.o.termguicolors and #vim.api.nvim_list_uis() > 0
end

local function send(data)
  if vim.api.nvim_ui_send then
    vim.api.nvim_ui_send(data)
  else
    io.stdout:write(data)
  end
end

--- Sends a PNG to kitty under `id`, replacing any image with that id, and
--- makes a virtual placement `cols` × `rows` cells for placeholders to show.
function M.transmit(id, png, cols, rows)
  local data = vim.base64.encode(png)
  local chunk = 4096
  local first = true
  for i = 1, #data, chunk do
    local part = data:sub(i, i + chunk - 1)
    local more = i + chunk <= #data and 1 or 0
    local keys = first and ("a=T,U=1,f=100,q=2,i=%d,c=%d,r=%d,m=%d"):format(id, cols, rows, more) or ("m=%d"):format(more)
    send("\27_G" .. keys .. ";" .. part .. "\27\\")
    first = false
  end
end

--- Forgets an image.
function M.delete(id)
  send(("\27_Ga=d,d=I,i=%d,q=2\27\\"):format(id))
end

--- The placeholder text for one row of an image.
function M.row(r, cols)
  local mark = vim.fn.nr2char(DIACRITICS[r + 1] or DIACRITICS[#DIACRITICS])
  return PLACEHOLDER .. mark .. vim.fn.nr2char(DIACRITICS[1]) .. string.rep(PLACEHOLDER, cols - 1)
end

local function hex(group, attr, fallback)
  local hl = vim.api.nvim_get_hl(0, { name = group, link = false })
  local v = hl[attr]
  return v and ("#%06x"):format(v) or fallback
end

--- Colours for charts, from the colour scheme.
function M.style(width, height)
  return {
    width = width,
    height = height,
    fg = hex("DenChart", "fg", hex("Normal", "fg", "#c0c0c0")),
    muted = hex("DenMuted", "fg", "#707070"),
    accent = hex("DenTimer", "fg", "#e0a060"),
    extra = hex("DenDone", "fg", "#80b080"),
  }
end

-- Pixels per cell; kitty scales the image into the cells either way, this
-- just keeps it sharp.
local CELL_W, CELL_H = 18, 38

--- Draws a chart and appends its placeholder rows to `lines` (screen lines
--- from den.ui.screen). Returns false when images are off or drawing fails.
---@param kind "bars"|"burndown"|"rings"
function M.lines(name, kind, data, cols, rows)
  if not M.enabled() then
    return nil
  end
  local id = id_for(name)
  local ok, png = pcall(native.need().chart_png, kind, data, M.style(cols * CELL_W, rows * CELL_H))
  if not ok then
    return nil
  end
  -- A redraw with the same picture does not resend it.
  local key = cols .. "x" .. rows .. ":" .. png
  if sent[id] ~= key then
    M.transmit(id, png, cols, rows)
    sent[id] = key
  end
  local group = ("DenImage%06x"):format(id)
  vim.api.nvim_set_hl(0, group, { fg = ("#%06x"):format(id) })
  local out = {}
  for r = 0, rows - 1 do
    table.insert(out, { text = M.row(r, cols), group = group })
  end
  return out
end

--- For screens: draws a named chart and pushes its rows, indented, onto a
--- screen's `lines`. `data` is engine data for the kind of chart.
function M.place(lines, kind, data, cols, rows)
  local S = require("den.ui.screen")
  local chart, name
  if kind == "closed" then
    local values = {}
    for _, d in ipairs(data) do
      table.insert(values, d.count)
    end
    chart, name = { values = values }, "closed"
    kind = "bars"
  elseif kind == "burndown" then
    local values = {}
    for _, p in ipairs(data.points) do
      table.insert(values, p.count)
    end
    local days = (require("den.util").days_between(data.start, data.due) or 0) + 1
    chart, name = { values = values, days = days }, "burndown:" .. data.project.name
  else
    chart, name = data, kind
  end
  local placed = M.lines(name, kind, chart, cols, rows)
  if not placed then
    return false
  end
  for _, row in ipairs(placed) do
    local l = S.add(S.line(), "    ")
    S.add(l, row.text, row.group)
    table.insert(lines, l)
  end
  return true
end

return M
