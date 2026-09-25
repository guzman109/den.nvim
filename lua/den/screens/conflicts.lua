-- The Conflicts screen: where two machines changed the same lines.
--
-- Sync already combined every conflict it safely could (task edits that do
-- not overlap). What is left shows here, one block per conflict, with both
-- versions side by side in words: this machine's and the other one's. Pick
-- one, keep both, take the combined version when there is one, or edit the
-- file by hand. When nothing is left, the sync finishes on its own.

local S = require("den.ui.screen")
local rows = require("den.ui.rows")
local native = require("den.native")
local apply = require("den.apply")

local M = {}

local NAME = "conflicts"

local function files()
  return require("den.sync").conflicted()
end

local function block(push, label, lines, hl, item)
  push(S.add(S.add(S.line(), "    "), label, hl), item)
  if #lines == 0 then
    push(S.add(S.line(), "      (nothing)", "DenMuted"), item)
  end
  for _, text in ipairs(lines) do
    push(S.add(S.line(), "      " .. text), item)
  end
end

local function render(ctx, width)
  local lines, items = {}, {}
  local function push(line, item)
    table.insert(lines, line)
    if item then
      items[#lines] = item
    end
  end
  local total = 0
  local list = {}
  ctx.settled = ctx.settled or {}
  for _, path in ipairs(files()) do
    local info = not ctx.settled[path] and native.call("conflicts", path)
    if info then
      table.insert(list, info)
      total = total + (info.locked and 1 or #info.hunks)
    end
  end
  ctx.total = total
  push(rows.header("Conflicts", total == 1 and "1 to settle" or (total .. " to settle"), width))
  if total == 0 then
    push(S.line())
    push(S.add(S.line(), "  Everything is settled. Press s to finish the sync.", "DenMuted"))
  end
  for _, info in ipairs(list) do
    local other = info.other_name and ("other machine (" .. info.other_name .. ")") or "other machine"
    if info.locked then
      local item = { key = info.path, path = info.path, locked = true }
      push(S.line())
      push(S.add(S.add(S.line(), "  "), info.path, "DenGroup"), item)
      push(S.add(S.line(), "    A locked note both machines changed in the same places.", "DenMuted"), item)
      push(S.add(S.line(), "    m keeps this machine's version, t keeps the " .. other .. "'s.", "DenMuted"), item)
    end
    for index, hunk in ipairs(info.hunks) do
      local item = { key = info.path .. "\0" .. index, path = info.path, index = index, count = #info.hunks, hunk = hunk }
      push(S.line())
      push(S.add(S.add(S.line(), "  "), ("%s · line %d"):format(info.path, hunk.start + 1), "DenGroup"), item)
      block(push, other, hunk.other, "DenMuted", item)
      block(push, "this machine", hunk.mine, "DenMuted", item)
      if hunk.base then
        block(push, "before either", hunk.base, "DenMuted", item)
      end
      if hunk.combined then
        block(push, "combined", hunk.combined, "DenDone", item)
      end
    end
  end
  push(S.line())
  push(rows.hints({
    { "c", "combine" },
    { "m", "mine" },
    { "t", "theirs" },
    { "b", "both" },
    { "e", "edit" },
    { "g?", "keys" },
  }))
  return { lines = lines, items = items }
end

--- Settles one conflict and redraws; finishes the sync when none are left.
local function settle(choice)
  return function(item, ctx)
    if not item or not item.path then
      return
    end
    if item.locked then
      if choice ~= "mine" and choice ~= "other" then
        vim.notify("Den: a locked note can only be kept whole: m mine, t theirs", vim.log.levels.WARN)
        return
      end
      local _, why = native.call("take_side", item.path, choice)
      if why then
        vim.notify("Den: " .. why, vim.log.levels.ERROR)
        return
      end
      ctx.settled[item.path] = true
      S.render(NAME)
      if (ctx.total or 0) == 0 then
        require("den.sync").run({ resume = true })
      end
      return
    end
    if choice == "combine" and not item.hunk.combined then
      vim.notify("Den: these edits overlap; pick one side, keep both, or edit by hand", vim.log.levels.WARN)
      return
    end
    local choices = {}
    for i = 1, item.count do
      choices[i] = i == item.index and choice or "leave"
    end
    if not apply.run(function(mod)
      return mod.plan_resolve(item.path, choices)
    end) then
      return
    end
    S.render(NAME)
    if (ctx.total or 0) == 0 then
      require("den.sync").run({ resume = true })
    end
  end
end

local keys
keys = {
  c = { settle("combine"), "take the combined version" },
  m = { settle("mine"), "keep this machine's version" },
  t = { settle("other"), "keep the other machine's version" },
  b = { settle("both"), "keep both, the other machine's first" },
  e = {
    function(item)
      if item and item.locked then
        vim.notify("Den: a locked note has no lines to edit here; keep one version with m or t", vim.log.levels.WARN)
      elseif item and item.path then
        vim.cmd.edit(vim.fn.fnameescape(native.call("abs", item.path)))
        pcall(vim.api.nvim_win_set_cursor, 0, { item.hunk.start + 1, 0 })
      end
    end,
    "edit the file by hand at this conflict",
  },
  s = {
    function()
      require("den.sync").run({ resume = true })
    end,
    "finish the sync",
  },
  q = {
    function()
      local alt = vim.fn.bufnr("#")
      if alt ~= -1 and vim.api.nvim_buf_is_valid(alt) then
        vim.api.nvim_set_current_buf(alt)
      else
        vim.cmd("enew")
      end
    end,
    "leave the screen",
  },
  ["g?"] = {
    function()
      S.help({ keys = keys })
    end,
    "show keys",
  },
}

function M.open()
  S.open(NAME, { render = render, keys = keys, ctx = {} })
end

M.render = render
M.keys = keys

return M
