-- Locked notes, end to end: a real den-agent on a private socket, a vault
-- key made with a fast password hash, and notes locked, edited, saved,
-- forgotten and opened again.

local native = require("den.native")
local locked = require("den.locked")

local answers = {}
local real_inputsecret = vim.fn.inputsecret
vim.fn.inputsecret = function(prompt)
  local a = table.remove(answers, 1)
  if a == nil then
    error("unexpected question: " .. tostring(prompt))
  end
  return a
end

local function lock_events_settle(pred)
  T.ok(vim.wait(10000, pred, 20), "the agent answers")
  T.settle(50)
end

local function write(rel, text)
  local f = assert(io.open(T.vault .. "/" .. rel, "w"))
  f:write(text)
  f:close()
end

local function exists(rel)
  return vim.uv.fs_stat(T.vault .. "/" .. rel) ~= nil
end

T.test("setup makes the vault key and shows the recovery key once", function()
  answers = { "correct horse", "correct horse" }
  vim.cmd("Den lock setup")
  lock_events_settle(function()
    local s = locked.status()
    return s ~= nil and s.set_up and s.unlocked
  end)
  lock_events_settle(function()
    return vim.bo.buftype == "nofile" and table.concat(T.lines(), "\n"):find("AGE%-SECRET%-KEY%-1") ~= nil
  end)
  T.contains(T.lines(), "It is shown only now")
  vim.cmd("normal q")
  T.ok(exists(".den/keys/recipient") and exists(".den/keys/password.age") and exists(".den/keys/recovery.age"))
end)

T.test("locking a note encrypts it and keeps it readable in memory only", function()
  write("notes/diary.md", "# Diary\n\n- [ ] Tell nobody about the surprise\n")
  native.call("reload", "notes/diary.md")
  vim.cmd.edit(T.vault .. "/notes/diary.md")
  vim.cmd("Den lock note")
  T.ok(not exists("notes/diary.md"), "the plain file is gone")
  T.ok(exists("notes/diary.md.age"))
  T.lacks(T.read("notes/diary.md.age"), "surprise")
  T.contains(T.read("notes/diary.md.age"), "BEGIN AGE ENCRYPTED FILE")
  T.contains(T.lines(), "Tell nobody about the surprise")
  T.ok(not vim.bo.swapfile, "no swap file")
  T.ok(not vim.bo.undofile, "no undo file")
  T.ok(vim.b.den_locked)
  T.contains(vim.o.shada, "<0")
  T.contains(vim.o.shada, "/0")
end)

T.test("saving encrypts again, and nothing reaches the engine's views", function()
  vim.cmd.edit(T.vault .. "/notes/diary.md.age")
  T.contains(T.lines(), "Tell nobody about the surprise")
  vim.api.nvim_buf_set_lines(0, -1, -1, false, { "- [ ] Buy the secret cake" })
  require("den.apply").sync_overlays()
  local view = native.call("tasks_view", nil)
  local all = vim.inspect(view)
  T.lacks(all, "secret cake")
  T.lacks(all, "surprise")
  vim.cmd("write")
  T.ok(not vim.bo.modified)
  T.lacks(T.read("notes/diary.md.age"), "cake")
  local got = native.call("lock_read", "notes/diary.md.age")
  T.contains(got.text, "- [ ] Buy the secret cake\n")
end)

T.test("locking forgets the key and closes locked notes", function()
  vim.cmd.edit(T.vault .. "/notes/diary.md.age")
  T.ok(vim.b.den_locked)
  vim.cmd("Den lock")
  T.ok(not locked.status().unlocked)
  for _, buf in ipairs(vim.api.nvim_list_bufs()) do
    T.ok(not (vim.api.nvim_buf_is_loaded(buf) and vim.b[buf].den_locked), "no locked buffer stays open")
  end
  local _, why = native.call("lock_read", "notes/diary.md.age")
  T.contains(tostring(why), "locked")
end)

T.test("opening a locked note asks for the password, then shows it", function()
  answers = { "wrong horse" }
  vim.cmd.edit(T.vault .. "/notes/diary.md.age")
  T.contains(T.lines(), "This note is locked")
  lock_events_settle(function()
    local last = T.notes[#T.notes]
    return last ~= nil and last:find("wrong password", 1, true) ~= nil
  end)
  T.contains(T.lines(), "This note is locked")
  answers = { "correct horse" }
  vim.cmd("Den unlock")
  lock_events_settle(function()
    -- While the password is being checked the agent is busy, and status
    -- says so rather than making Neovim wait.
    local s = locked.status()
    return s ~= nil and s.unlocked
  end)
  vim.cmd("edit!")
  T.contains(T.lines(), "Buy the secret cake")
end)

T.test("unlocking a note for good makes it plain again", function()
  vim.cmd.edit(T.vault .. "/notes/diary.md.age")
  vim.cmd("Den unlock note")
  T.ok(exists("notes/diary.md") and not exists("notes/diary.md.age"))
  T.contains(T.read("notes/diary.md"), "Buy the secret cake")
  T.ok(not vim.b.den_locked)
end)

T.test("the agent stops", function()
  vim.fn.inputsecret = real_inputsecret
  native.call("lock_call", { op = "stop" })
  T.ok(vim.wait(3000, function()
    return vim.uv.fs_stat(vim.env.DEN_AGENT_SOCKET) == nil
  end, 20), "the socket is gone")
end)
