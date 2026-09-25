-- Asking for a passphrase inside Neovim, for SSH and git.
--
-- A foreground sync runs git with Den's `den` binary as the askpass program.
-- When SSH needs a passphrase it runs `den "<prompt>"`, which connects to
-- this Neovim and calls `begin(prompt)`, then polls `result(id)` until the
-- person answers. Secrets are read with `inputsecret()`, so they are never
-- shown, kept in history, or written to a file; the answer is dropped as
-- soon as it has been collected.

local M = {}

local pending = {}
local next_id = 0

--- Whether a prompt asks for something secret (rather than yes or no).
local function secret(prompt)
  local p = prompt:lower()
  return p:find("passphrase", 1, true) or p:find("password", 1, true) or p:find("%f[%a]pin%f[%A]") ~= nil
end

--- Starts asking. Returns an id for `result()`.
function M.begin(prompt)
  next_id = next_id + 1
  local id = next_id
  pending[id] = { done = false }
  vim.schedule(function()
    local entry = pending[id]
    if not entry then
      return
    end
    local text = vim.trim(prompt or "")
    local ask = secret(text) and vim.fn.inputsecret or vim.fn.input
    local cancelled = "\0cancelled"
    local ok, value = pcall(ask, { prompt = "Den · " .. text .. " ", cancelreturn = cancelled })
    vim.cmd("redraw")
    if pending[id] then
      pending[id] = { done = true, ok = ok and value ~= cancelled, value = ok and value ~= cancelled and value or nil }
    end
  end)
  return id
end

--- The answer once given: `{ ok = true, value = "…" }` or `{ ok = false }`,
--- and nil while still waiting.
function M.result(id)
  local entry = pending[id]
  if not entry or not entry.done then
    return nil
  end
  pending[id] = nil
  return { ok = entry.ok, value = entry.value }
end

--- Gives up on a question (the askpass timed out).
function M.cancel(id)
  pending[id] = nil
  return true
end

return M
