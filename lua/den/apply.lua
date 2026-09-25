-- Applying the engine's planned changes.
--
-- A change says what a file must contain now (`before`) and what it should
-- contain after (`after`). When the file is open in a buffer, the change is
-- made to the buffer — so unsaved edits are kept — and a buffer that had no
-- unsaved edits is written straight away. Files that aren't open are written
-- by the engine, atomically.

local native = require("den.native")
local state = require("den.state")

local M = {}

--- Vault buffers Den has told the engine about, by vault path.
local overlaid = {}

local function buf_for(abs)
  local buf = vim.fn.bufnr(abs)
  if buf ~= -1 and vim.api.nvim_buf_is_loaded(buf) then
    return buf
  end
  return nil
end

--- A buffer's text, ending in a newline exactly when its file did.
function M.buf_text(buf)
  local lines = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  local nl = vim.bo[buf].fileformat == "dos" and "\r\n" or "\n"
  if #lines == 1 and lines[1] == "" then
    return ""
  end
  local text = table.concat(lines, nl)
  if vim.bo[buf].eol then
    text = text .. nl
  end
  return text
end

local function split(text)
  text = text:gsub("\r\n", "\n")
  local lines = vim.split(text, "\n", { plain = true })
  if lines[#lines] == "" then
    table.remove(lines)
  end
  return lines
end

--- Replaces only the lines that differ, so marks and folds elsewhere survive.
local function set_text(buf, text)
  local old = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  local new = split(text)
  local first = 1
  while first <= #old and first <= #new and old[first] == new[first] do
    first = first + 1
  end
  local old_last, new_last = #old, #new
  while old_last >= first and new_last >= first and old[old_last] == new[new_last] do
    old_last = old_last - 1
    new_last = new_last - 1
  end
  vim.api.nvim_buf_set_lines(buf, first - 1, old_last, false, vim.list_slice(new, first, new_last))
end

--- Tells the engine about unsaved edits in open vault files, so plans are
--- made against what is on screen.
function M.sync_overlays()
  local mod = native.get()
  if not mod or not state.ready then
    return
  end
  local seen = {}
  for _, buf in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_loaded(buf) and vim.bo[buf].buftype == "" then
      local name = vim.api.nvim_buf_get_name(buf)
      if name ~= "" then
        local ok, rel = pcall(mod.rel, name)
        if ok and rel then
          if vim.bo[buf].modified then
            pcall(mod.set_overlay, rel, M.buf_text(buf))
            overlaid[rel] = true
            seen[rel] = true
          end
        end
      end
    end
  end
  for rel in pairs(overlaid) do
    if not seen[rel] then
      pcall(mod.set_overlay, rel, nil)
      overlaid[rel] = nil
    end
  end
end

--- Forgets an overlay (after the buffer was saved or closed).
function M.clear_overlay(rel)
  local mod = native.get()
  if mod and overlaid[rel] then
    pcall(mod.set_overlay, rel, nil)
    overlaid[rel] = nil
  end
end

--- Applies changes. Returns true, or false and a message.
function M.apply(changes)
  local mod = native.need()
  local disk, paths = {}, {}
  for _, change in ipairs(changes or {}) do
    table.insert(paths, change.path)
    local buf = buf_for(mod.abs(change.path))
    if buf then
      if change.before and M.buf_text(buf) ~= change.before then
        return false, change.path .. " changed on screen since Den read it; try again"
      end
      local was_modified = vim.bo[buf].modified
      set_text(buf, change.after)
      if not was_modified then
        vim.api.nvim_buf_call(buf, function()
          vim.cmd("silent! noautocmd write")
        end)
        M.clear_overlay(change.path)
        pcall(mod.reload, change.path)
      else
        pcall(mod.set_overlay, change.path, M.buf_text(buf))
        overlaid[change.path] = true
      end
    else
      table.insert(disk, change)
    end
  end
  if #disk > 0 then
    local ok, err = pcall(mod.apply, disk)
    if not ok then
      return false, (tostring(err):gsub("^runtime error: ", ""))
    end
  end
  state.changed(paths)
  return true
end

--- Plans with `fn(mod)` against the current screen, then applies. Reports
--- failures to the person; returns true on success.
function M.run(fn)
  M.sync_overlays()
  local mod = native.need()
  local ok, changes = pcall(fn, mod)
  if not ok then
    vim.notify("Den: " .. tostring(changes):gsub("^runtime error: ", ""), vim.log.levels.WARN)
    return false
  end
  local done, err = M.apply(changes)
  if not done then
    vim.notify("Den: " .. err, vim.log.levels.WARN)
    return false
  end
  return true
end

return M
