-- The Inbox screen: every capture not yet sorted, grouped by project.

local S = require("den.ui.screen")
local rows = require("den.ui.rows")
local actions = require("den.actions")
local native = require("den.native")
local state = require("den.state")

local M = {}

local NAME = "inbox"

local function render(ctx, width)
  local lines, items = {}, {}
  local function push(line, item)
    table.insert(lines, line)
    if item then
      items[#lines] = item
    end
  end
  if not state.ready then
    push(S.add(S.line(), "  Den is loading your vault…", "DenMuted"))
    return { lines = lines, items = items }
  end
  local mod = native.need()
  local view = mod.inbox_view(ctx.project)
  local draw = { today = mod.today(), running = mod.timer_running() }
  local all = {}
  for _, g in ipairs(view.groups) do
    vim.list_extend(all, g.tasks)
  end
  local cols = rows.columns(width, all, false)
  local title = ctx.project and ("Inbox · " .. (ctx.title or ctx.project)) or "Inbox"
  push(rows.header(title, view.count == 1 and "1 to sort" or (view.count .. " to sort"), width))
  if view.count == 0 then
    push(S.line())
    push(S.add(S.line(), "  Nothing to sort. Captures land here.", "DenMuted"))
  end
  for _, group in ipairs(view.groups) do
    push(S.line())
    push(S.add(S.line(), "  " .. group.label, group.project and "DenGroup" or "DenMuted"))
    for _, row in ipairs(group.tasks) do
      row.key = rows.key(row)
      push(rows.line(row, cols, draw, false), row)
    end
  end
  push(S.line())
  push(rows.hints({
    { "m", "move to project" },
    { ">", "make it a next action here" },
    { "-", "drop" },
    { "e", "edit" },
    { "o", "open file" },
    { "g?", "keys" },
  }))
  return { lines = lines, items = items }
end

local function task(fn)
  return function(item, ctx)
    if item and item.path then
      fn(item, ctx)
    end
  end
end

local keys
keys = {
  m = { task(actions.move_pick), "move to another project" },
  [">"] = {
    task(function(item)
      if item.project then
        actions.move(item, nil, "next_actions")
      else
        actions.move_pick(item)
      end
    end),
    "make it a next action in its project",
  },
  ["-"] = { task(actions.drop), "drop" },
  x = { task(actions.done), "done" },
  s = { task(actions.start), "start it now" },
  e = { task(actions.edit), "edit the text" },
  o = { task(actions.open), "open the file" },
  ["<CR>"] = { task(actions.open), "open the file" },
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

function M.open(opts)
  opts = opts or {}
  S.open(NAME, { render = render, keys = keys, ctx = { project = opts.project } })
end

M.render = render
M.keys = keys

return M
