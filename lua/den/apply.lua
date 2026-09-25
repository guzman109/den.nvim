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

--- A path with its links resolved, so `/var/…` and `/private/var/…` match.
--- A file that doesn't exist yet resolves through its folder.
local function real(path)
  local resolved = vim.uv.fs_realpath(path)
  if resolved then
    return resolved
  end
  local folder = vim.uv.fs_realpath(vim.fs.dirname(path))
  if folder then
    return folder .. "/" .. vim.fs.basename(path)
  end
  return vim.fs.normalize(path)
end

--- The loaded buffer holding exactly this file. (`bufnr()` would treat the
--- name as a pattern and could pick a different file.)
local function buf_for(abs)
  local want = real(abs)
  for _, buf in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_loaded(buf) and vim.bo[buf].buftype == "" then
      local name = vim.api.nvim_buf_get_name(buf)
      if name ~= "" and (name == abs or real(name) == want) then
        return buf
      end
    end
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
    -- Locked notes never tell the engine what they hold.
    if vim.api.nvim_buf_is_loaded(buf) and vim.bo[buf].buftype == "" and not vim.b[buf].den_locked then
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
---
--- Every change is checked before any is made (the buffer's text for open
--- files, the file on disk for the rest), so a plan that touches two files,
--- like a move, is applied whole or not at all. Then the changes are made in
--- the plan's order.
function M.apply(changes)
  local mod = native.need()
  local steps, check, paths = {}, {}, {}
  for _, change in ipairs(changes or {}) do
    table.insert(paths, change.path)
    local buf = buf_for(mod.abs(change.path))
    local step = { change = change, buf = buf }
    if buf and (change.delete or vim.b[buf].den_locked) then
      -- A file going away, or a locked note (the buffer holds plaintext,
      -- the change ciphertext): the engine writes the disk, and the buffer
      -- is closed or read again.
      if vim.bo[buf].modified then
        return false, change.path .. " has unsaved changes; save or undo them first"
      end
      step.disk = true
      step.after = change.delete and "wipe" or "reread"
      table.insert(check, change)
    elseif buf then
      if change.before and M.buf_text(buf) ~= change.before then
        return false, change.path .. " changed on screen since Den read it; try again"
      end
      step.modified = vim.bo[buf].modified
      if not step.modified then
        -- Written straight away, over the file on disk: that must still
        -- hold what the buffer shows.
        table.insert(check, change)
      end
    else
      step.disk = true
      table.insert(check, change)
    end
    table.insert(steps, step)
  end
  if #check > 0 then
    local ok, err = pcall(mod.check, check)
    if not ok then
      return false, native.message(err)
    end
  end

  local wipe, reread = {}, {}
  for _, step in ipairs(steps) do
    local change, buf = step.change, step.buf
    if step.disk then
      local ok, err = pcall(mod.apply, { change })
      if not ok then
        state.changed(paths)
        return false, native.message(err)
      end
      if step.after == "wipe" then
        table.insert(wipe, buf)
      elseif step.after == "reread" then
        table.insert(reread, buf)
      end
    else
      set_text(buf, change.after)
      if not step.modified then
        vim.api.nvim_buf_call(buf, function()
          vim.cmd("silent! noautocmd write")
        end)
      end
      if vim.bo[buf].modified then
        -- Kept unsaved, as the person left it (or the write failed).
        pcall(mod.set_overlay, change.path, M.buf_text(buf))
        overlaid[change.path] = true
      else
        M.clear_overlay(change.path)
        pcall(mod.reload, change.path)
      end
    end
  end
  for _, buf in ipairs(wipe) do
    pcall(vim.api.nvim_buf_delete, buf, { force = true })
  end
  for _, buf in ipairs(reread) do
    local rel = mod.rel(vim.api.nvim_buf_get_name(buf))
    if rel then
      require("den.locked").read(buf, rel)
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
    vim.notify("Den: " .. native.message(changes), vim.log.levels.WARN)
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
