-- Locked notes in Neovim.
--
-- Opening a `*.md.age` file in the vault asks den-agent to decrypt it into
-- a buffer that never touches the disk in the clear: no swap file, no undo
-- file, and for the rest of the session no registers or search history in
-- ShaDa. Saving encrypts the text again (that needs only the vault's public
-- key) and writes the file atomically. The buffer never tells the engine
-- what it holds, so locked tasks stay out of every screen.
--
-- When the vault is locked, the buffer shows a short note and Den asks to
-- unlock: Touch ID first when it is set up on this Mac, else the password.

local native = require("den.native")
local state = require("den.state")

local M = {}

local protected = false
local waiting = {}

--- The agent program shipped with the plugin.
local function agent_program()
  local own = native.root .. "/bin/den-agent"
  if vim.fn.executable(own) == 1 then
    return own
  end
  local found = vim.fn.exepath("den-agent")
  return found ~= "" and found or own
end

local function call(request)
  native.call("lock_agent_program", agent_program())
  return native.call("lock_call", request)
end

local function call_async(request)
  native.call("lock_agent_program", agent_program())
  return native.call("lock_call_async", request)
end

--- Keeps what was read from locked notes out of ShaDa for this session.
function M.protect()
  if protected then
    return
  end
  protected = true
  local kept = {}
  for _, part in ipairs(vim.split(vim.o.shada, ",", { trimempty = true })) do
    local c = part:sub(1, 1)
    if c ~= "<" and c ~= '"' and c ~= "/" then
      table.insert(kept, part)
    end
  end
  table.insert(kept, "<0")
  table.insert(kept, "/0")
  vim.o.shada = table.concat(kept, ",")
end

--- `{ set_up, unlocked, methods, strict }`, or nil and why.
function M.status()
  return call({ op = "status" })
end

local function lines_of(text)
  local lines = vim.split((text:gsub("\r\n", "\n")), "\n", { plain = true })
  local eol = lines[#lines] == ""
  if eol then
    table.remove(lines)
  end
  return lines, eol
end

local function memory_only(buf)
  vim.bo[buf].swapfile = false
  vim.bo[buf].undofile = false
  vim.b[buf].den_locked = true
end

--- Fills a buffer from its locked file; when the vault is locked, shows a
--- note and asks to unlock.
function M.read(buf, rel)
  memory_only(buf)
  M.protect()
  local got, why = native.call("lock_read", rel)
  why = tostring(why):match("^[^\n]*")
  if got then
    local lines, eol = lines_of(got.text)
    vim.bo[buf].modifiable = true
    vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
    vim.bo[buf].eol = eol
    vim.bo[buf].modified = false
    vim.bo[buf].filetype = "markdown"
    vim.b[buf].den_armored = got.armored
    return true
  end
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, {
    "🔒 This note is locked.",
    "",
    ":Den unlock opens it. (" .. why .. ")",
  })
  vim.bo[buf].modifiable = false
  vim.bo[buf].modified = false
  vim.b[buf].den_armored = nil
  if why:find("locked", 1, true) then
    M.unlock(function(ok)
      if ok and vim.api.nvim_buf_is_valid(buf) then
        M.read(buf, rel)
      end
    end)
  end
  return false
end

--- Encrypts the buffer and writes it.
function M.write(buf, rel)
  if vim.b[buf].den_armored == nil and vim.fn.filereadable(native.call("abs", rel)) == 1 then
    vim.notify("Den: this note has not been opened unlocked, so it is not saved", vim.log.levels.WARN)
    return false
  end
  local lines = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  local text = table.concat(lines, "\n") .. (vim.bo[buf].eol and "\n" or "")
  local changes, why = native.call("plan_write_locked", rel, vim.b[buf].den_armored, text)
  if not changes then
    vim.notify("Den: " .. tostring(why), vim.log.levels.ERROR)
    return false
  end
  local ok, err = pcall(native.need().apply, changes)
  if not ok then
    vim.notify("Den: " .. tostring(err):gsub("^runtime error: ", ""), vim.log.levels.ERROR)
    return false
  end
  vim.b[buf].den_armored = changes[1].after
  vim.bo[buf].modified = false
  state.changed({ rel })
  return true
end

local function finish(ok, message)
  local done = waiting
  waiting = {}
  for _, fn in ipairs(done) do
    pcall(fn, ok, message)
  end
end

--- Unlocks the vault: Touch ID when set up on this Mac, else the password.
--- `on_done(ok)` runs after.
function M.unlock(on_done, method)
  local s, why = M.status()
  if not s then
    vim.notify("Den: " .. tostring(why), vim.log.levels.ERROR)
    return on_done and on_done(false)
  end
  if not s.set_up then
    vim.notify("Den: locking is not set up for this vault (:Den lock setup)")
    return on_done and on_done(false)
  end
  if s.unlocked then
    return on_done and on_done(true)
  end
  if on_done then
    table.insert(waiting, on_done)
  end
  if #waiting > 1 then
    return -- already asking
  end
  method = method or (vim.tbl_contains(s.methods, "touch_id") and "touch_id" or "password")
  if method == "password" or method == "recovery" then
    local secret = vim.fn.inputsecret(method == "password" and "Vault password: " or "Recovery key: ")
    vim.cmd("redraw")
    if secret == "" then
      return finish(false)
    end
    vim.notify("Den: unlocking…")
    call_async({ op = "unlock", method = method, secret = secret })
  else
    vim.notify(method == "touch_id" and "Den: touch the sensor to unlock" or "Den: touch your YubiKey")
    call_async({ op = "unlock", method = method })
  end
end

local function on_events(events)
  for _, ev in ipairs(events) do
    if ev.kind == "lock" then
      if ev.op == "unlock" then
        if ev.ok then
          vim.notify("Den: unlocked")
          finish(true)
        elseif #waiting > 0 and vim.tbl_contains(M.status() and M.status().methods or {}, "password") and ev.error and ev.error:find("Touch ID", 1, true) then
          -- Touch ID failed or was cancelled: fall back to the password.
          local keep = waiting
          waiting = {}
          M.unlock(function(ok)
            for _, fn in ipairs(keep) do
              pcall(fn, ok)
            end
          end, "password")
        else
          vim.notify("Den: " .. tostring(ev.error), vim.log.levels.WARN)
          finish(false, ev.error)
        end
      elseif ev.op == "setup" then
        if ev.ok then
          M.show_recovery(ev.text)
        else
          vim.notify("Den: " .. tostring(ev.error), vim.log.levels.ERROR)
        end
      elseif ev.ok then
        vim.notify(({
          set_password = "Den: password changed",
          enable_touch_id = "Den: Touch ID can unlock the vault on this Mac",
          add_yubikey = "Den: your YubiKey can unlock the vault",
        })[ev.op] or "Den: done")
      else
        vim.notify("Den: " .. tostring(ev.error), vim.log.levels.WARN)
      end
    end
  end
end

--- Shows the recovery key once, in a window that is gone when closed.
function M.show_recovery(key)
  local lines = {
    "",
    "  Locking is set up, and the vault is unlocked.",
    "",
    "  This is your recovery key. Write it down or print it and keep it",
    "  somewhere safe, away from this computer. It is shown only now, and",
    "  it is the only way back in if the password is lost.",
    "",
    "    " .. tostring(key),
    "",
    "  q closes this window.",
    "",
  }
  local b = vim.api.nvim_create_buf(false, true)
  vim.bo[b].swapfile = false
  vim.bo[b].undofile = false
  vim.bo[b].bufhidden = "wipe"
  vim.api.nvim_buf_set_lines(b, 0, -1, false, lines)
  vim.bo[b].modifiable = false
  local width = 76
  vim.api.nvim_open_win(b, true, {
    relative = "editor",
    row = math.floor((vim.o.lines - #lines) / 2),
    col = math.floor((vim.o.columns - width) / 2),
    width = width,
    height = #lines,
    style = "minimal",
    border = "rounded",
    title = " recovery key ",
    title_pos = "center",
  })
  vim.keymap.set("n", "q", function()
    vim.api.nvim_buf_delete(b, { force = true })
  end, { buffer = b, nowait = true })
end

--- `:Den lock setup`.
function M.setup_keys()
  local s = M.status()
  if s and s.set_up then
    vim.notify("Den: locking is already set up for this vault")
    return
  end
  local first = vim.fn.inputsecret("New vault password: ")
  local again = first ~= "" and vim.fn.inputsecret("Again: ") or ""
  vim.cmd("redraw")
  if first == "" then
    return
  end
  if first ~= again then
    vim.notify("Den: the passwords differ", vim.log.levels.WARN)
    return
  end
  vim.notify("Den: creating the vault key…")
  call_async({ op = "setup", password = first })
  -- This clone's git learns to diff and merge locked notes.
  local den = require("den.sync").program()
  if den then
    vim.system({ den, "lock", "git", "--vault", state.info.root }, { text = true })
  end
end

--- `:Den lock password`.
function M.change_password()
  local first = vim.fn.inputsecret("New vault password: ")
  local again = first ~= "" and vim.fn.inputsecret("Again: ") or ""
  vim.cmd("redraw")
  if first == "" or first ~= again then
    vim.notify("Den: " .. (first == "" and "nothing changed" or "the passwords differ"))
    return
  end
  call_async({ op = "set_password", password = first })
end

--- `:Den lock touch-id`.
function M.enable_touch_id()
  call_async({ op = "enable_touch_id" })
end

--- `:Den lock`: forget the key now, and close locked buffers.
function M.forget()
  call({ op = "lock" })
  for _, buf in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_loaded(buf) and vim.b[buf].den_locked then
      if vim.bo[buf].modified then
        vim.notify("Den: " .. vim.api.nvim_buf_get_name(buf) .. " has unsaved changes and stays open", vim.log.levels.WARN)
      else
        pcall(vim.api.nvim_buf_delete, buf, { force = true })
      end
    end
  end
  vim.notify("Den: locked")
end

--- `:Den lock note`: locks the note in the current buffer.
function M.lock_note()
  local buf = vim.api.nvim_get_current_buf()
  local rel = native.call("rel", vim.api.nvim_buf_get_name(buf))
  if not rel or rel:match("%.age$") then
    vim.notify("Den: this is not an unlocked vault note")
    return
  end
  if vim.bo[buf].modified then
    vim.notify("Den: save the note first", vim.log.levels.WARN)
    return
  end
  if require("den.apply").run(function(mod)
    return mod.plan_lock(rel)
  end) then
    vim.cmd.edit(vim.fn.fnameescape(native.call("abs", rel .. ".age")))
    vim.notify("Den: locked. Earlier versions stay in git history in the clear, if they were synced.")
  end
end

--- `:Den unlock note`: turns the locked note in the current buffer plain.
function M.unlock_note()
  local buf = vim.api.nvim_get_current_buf()
  local rel = native.call("rel", vim.api.nvim_buf_get_name(buf))
  if not rel or not rel:match("%.md%.age$") then
    vim.notify("Den: this is not a locked note")
    return
  end
  if vim.bo[buf].modified then
    vim.notify("Den: save the note first", vim.log.levels.WARN)
    return
  end
  local changes, why = native.call("plan_unlock_note", rel)
  if not changes then
    vim.notify("Den: " .. tostring(why), vim.log.levels.WARN)
    return
  end
  local ok, err = pcall(native.need().apply, changes)
  if not ok then
    vim.notify("Den: " .. tostring(err):gsub("^runtime error: ", ""), vim.log.levels.ERROR)
    return
  end
  local plain = rel:gsub("%.age$", "")
  vim.cmd.edit(vim.fn.fnameescape(native.call("abs", plain)))
  pcall(vim.api.nvim_buf_delete, buf, { force = true })
  state.changed({ rel, plain })
  vim.notify("Den: " .. plain .. " is a plain note again")
end

function M.setup()
  local group = vim.api.nvim_create_augroup("den.locked", { clear = true })
  local function vault_rel(file)
    if not state.info then
      return nil
    end
    return native.call("rel", vim.fn.fnamemodify(file, ":p"))
  end
  vim.api.nvim_create_autocmd("BufReadCmd", {
    group = group,
    pattern = "*.md.age",
    callback = function(ev)
      local rel = vault_rel(ev.match)
      if not rel then
        -- Not in the vault: the file as it is.
        local ok, lines = pcall(vim.fn.readfile, ev.match)
        vim.api.nvim_buf_set_lines(ev.buf, 0, -1, false, ok and lines or {})
        vim.bo[ev.buf].modified = false
        return
      end
      if state.ready then
        M.read(ev.buf, rel)
      else
        memory_only(ev.buf)
        state.when_ready(function()
          M.read(ev.buf, rel)
        end)
      end
    end,
  })
  vim.api.nvim_create_autocmd("BufWriteCmd", {
    group = group,
    pattern = "*.md.age",
    callback = function(ev)
      local rel = vault_rel(ev.match)
      if rel then
        M.write(ev.buf, rel)
      elseif vim.fn.writefile(vim.api.nvim_buf_get_lines(ev.buf, 0, -1, false), ev.match) == 0 then
        vim.bo[ev.buf].modified = false
      end
    end,
  })
  if not M._listening then
    state.on_change(on_events)
    M._listening = true
  end
end

return M
