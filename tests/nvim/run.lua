-- Den's Neovim tests. Run with scripts/test-nvim.sh.
--
-- Each run works on a fresh copy of the fixture vault, with no user config
-- and a private state folder.

local root = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h:h:h")
-- Line by line, so nothing is lost when output goes to a file and Neovim exits.
io.stdout:setvbuf("line")
vim.opt.rtp:prepend(root)
vim.cmd("runtime plugin/den.lua")

local tmp = vim.fn.tempname()
local vault = tmp .. "/vault"
vim.fn.mkdir(vault, "p")
vim.fn.system({ "cp", "-R", root .. "/tests/vault/.", vault })
vim.env.XDG_STATE_HOME = tmp .. "/state"
vim.env.DEN_CONFIG = tmp .. "/no-config.yaml"
-- A private den-agent for this run, stopped at the end; never the real one.
vim.env.DEN_AGENT_SOCKET = tmp .. "/agent/agent.sock"
vim.env.DEN_TEST_SCRYPT_LOG_N = "10"
vim.o.columns, vim.o.lines = 120, 40

local failures, count = {}, 0

-- Messages go to a list the tests can read, not to the output.
local notes = {}
vim.notify = function(msg)
  table.insert(notes, msg)
end

_G.T = {
  root = root,
  vault = vault,
  tmp = tmp,
  notes = notes,
}

function T.test(name, fn)
  count = count + 1
  local ok, err = xpcall(fn, debug.traceback)
  if ok then
    io.write("ok    " .. name .. "\n")
  else
    table.insert(failures, name)
    io.write("FAIL  " .. name .. "\n" .. tostring(err):gsub("\n", "\n      ") .. "\n")
  end
  vim.cmd("silent! %bwipeout!")
end

function T.eq(got, want, what)
  if not vim.deep_equal(got, want) then
    error((what or "values differ") .. "\n  want: " .. vim.inspect(want) .. "\n  got:  " .. vim.inspect(got), 2)
  end
end

function T.ok(value, what)
  if not value then
    error(what or "expected a true value", 2)
  end
end

function T.read(rel)
  local f = assert(io.open(T.vault .. "/" .. rel, "r"))
  local text = f:read("*a")
  f:close()
  return text
end

function T.lines(buf)
  return vim.api.nvim_buf_get_lines(buf or 0, 0, -1, false)
end

function T.contains(haystack, needle, what)
  if type(haystack) == "table" then
    haystack = table.concat(haystack, "\n")
  end
  if not haystack:find(needle, 1, true) then
    error((what or "missing text") .. ": " .. needle .. "\n--- in ---\n" .. haystack, 2)
  end
end

function T.lacks(haystack, needle, what)
  if type(haystack) == "table" then
    haystack = table.concat(haystack, "\n")
  end
  if haystack:find(needle, 1, true) then
    error((what or "unexpected text") .. ": " .. needle .. "\n--- in ---\n" .. haystack, 2)
  end
end

--- The screen item whose title is `title`, and its line number.
function T.item(screen_name, title)
  local buf = vim.fn.bufnr("den://" .. screen_name)
  for row, line in ipairs(T.lines(buf)) do
    if line:find(title, 1, true) then
      vim.api.nvim_win_set_cursor(0, { row, 0 })
      local item = require("den.ui.screen").item(screen_name)
      if item and item.title == title then
        return item, row
      end
    end
  end
  error("no item " .. title .. " on " .. screen_name, 2)
end

--- Runs the engine's pending events and scheduled callbacks.
function T.settle(ms)
  vim.wait(ms or 150, function()
    return false
  end, 10)
end

--- Starts Den on a fresh copy of the fixture vault. Each test file calls it,
--- so files never see each other's changes.
local copies = 0
function T.fresh()
  copies = copies + 1
  local dir = tmp .. "/vault" .. copies
  vim.fn.mkdir(dir, "p")
  vim.fn.system({ "cp", "-R", root .. "/tests/vault/.", dir })
  T.vault = dir
  vim.cmd("silent! %bwipeout!")
  T.ok(require("den").setup({ vault = dir, machine = "test", ask_about_folders = false }), "setup")
  T.ok(require("den.state").wait(10000), "the vault loads")
end

-- DEN_TEST=locked runs only files whose name contains "locked".
local files = vim.fn.glob(root .. "/tests/nvim/test_*.lua", false, true)
table.sort(files)
if vim.env.DEN_TEST and vim.env.DEN_TEST ~= "" then
  files = vim.tbl_filter(function(f)
    return vim.fn.fnamemodify(f, ":t"):find(vim.env.DEN_TEST, 1, true) ~= nil
  end, files)
end
for _, file in ipairs(files) do
  io.write("# " .. vim.fn.fnamemodify(file, ":t") .. "\n")
  T.fresh()
  dofile(file)
end

if vim.uv.fs_stat(vim.env.DEN_AGENT_SOCKET) then
  pcall(require("den.native").call, "lock_call", { op = "stop" })
end
io.write(string.format("\n%d tests, %d failed\n", count, #failures))
io.stdout:flush()
vim.fn.delete(tmp, "rf")
os.exit(#failures == 0 and 0 or 1)
