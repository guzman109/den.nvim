-- The Tasks screen: what is in progress, then open work by project.

local S = require("den.ui.screen")
local rows = require("den.ui.rows")
local actions = require("den.actions")
local native = require("den.native")
local state = require("den.state")

local M = {}

local NAME = "tasks"

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
  local view = mod.tasks_view(ctx.project)
  local draw = { today = mod.today(), running = mod.timer_running() }
  local all = vim.list_extend(vim.list_extend({}, view.doing), view.closed)
  for _, g in ipairs(view.groups) do
    vim.list_extend(all, g.tasks)
  end
  local cols = rows.columns(width, all, ctx.project == nil)

  local title = ctx.project and ("Tasks · " .. (ctx.title or ctx.project)) or "Tasks · all projects"
  local counts = string.format("%d open · %d doing", view.open, #view.doing)
  if view.locked > 0 then
    counts = counts .. " · " .. view.locked .. " locked"
  end
  push(rows.header(title, counts, width))
  if ctx.filter then
    push(S.add(S.add(S.line(), "  filter ", "DenMuted"), ctx.filter, "DenKey"))
  end
  push(S.line())

  local shown = 0
  local doing = vim.tbl_filter(function(r)
    return rows.matches(r, ctx.filter)
  end, view.doing)
  if #doing > 0 then
    push(S.add(S.line(), "  doing", "DenGroupDoing"))
    for _, row in ipairs(doing) do
      row.key = rows.key(row)
      push(rows.line(row, cols, draw, true), row)
      shown = shown + 1
    end
  end
  for _, group in ipairs(view.groups) do
    local tasks = vim.tbl_filter(function(r)
      return rows.matches(r, ctx.filter)
    end, group.tasks)
    if #tasks > 0 then
      push(S.line())
      push(S.add(S.line(), "  " .. group.label, group.project and "DenGroup" or "DenMuted"))
      for _, row in ipairs(tasks) do
        row.key = rows.key(row)
        push(rows.line(row, cols, draw, false), row)
        shown = shown + 1
      end
    end
  end
  if #view.closed > 0 then
    push(S.line())
    local arrow = ctx.show_closed and "▾" or "▸"
    push(
      S.add(S.add(S.line(), "  " .. arrow .. " closed this week  ", "DenMuted"), tostring(#view.closed), "DenMuted"),
      { key = "closed", toggle = true }
    )
    if ctx.show_closed then
      for _, row in ipairs(view.closed) do
        row.key = rows.key(row)
        push(rows.line(row, cols, draw, ctx.project == nil), row)
      end
    end
  end
  if shown == 0 then
    push(S.line())
    local empty = ctx.filter and "  Nothing matches the filter." or "  Nothing open here. a adds a task."
    push(S.add(S.line(), empty, "DenMuted"))
  end
  push(S.line())
  push(rows.hints({
    { "s", "start" },
    { "p", "pause" },
    { "x", "done" },
    { "-", "drop" },
    { "a", "add" },
    { "o", "open" },
    { "f", "filter" },
    { "<Tab>", ctx.project and "all projects" or "this project" },
    { "g?", "keys" },
  }))
  return { lines = lines, items = items }
end

local function current_project()
  local p = native.call("project_for_dir", vim.fn.getcwd())
  return p
end

local keys
keys = {
  s = {
    function(item)
      if item and item.path then
        actions.start(item)
      end
    end,
    "start: mark doing and time it",
  },
  p = { actions.pause, "pause the timer" },
  x = {
    function(item)
      if item and item.path then
        actions.done(item)
      end
    end,
    "done (again to reopen)",
  },
  ["-"] = {
    function(item)
      if item and item.path then
        actions.drop(item)
      end
    end,
    "drop (again to reopen)",
  },
  a = {
    function(_, ctx)
      actions.add(ctx.project, "next_actions")
    end,
    "add a task",
  },
  e = {
    function(item)
      if item and item.path then
        actions.edit(item)
      end
    end,
    "edit the task's text",
  },
  m = {
    function(item)
      if item and item.path then
        actions.move_pick(item)
      end
    end,
    "move to another project",
  },
  o = {
    function(item, ctx)
      if item and item.toggle then
        ctx.show_closed = not ctx.show_closed
        S.render(NAME)
      elseif item and item.path then
        actions.open(item)
      end
    end,
    "open the task in its file",
  },
  ["<CR>"] = {
    function(item, ctx)
      keys.o[1](item, ctx)
    end,
    "open (or show closed tasks)",
  },
  f = {
    function(_, ctx)
      vim.ui.input({ prompt = "Filter: ", default = ctx.filter or "" }, function(text)
        if text == nil then
          return
        end
        ctx.filter = text ~= "" and text or nil
        S.render(NAME)
      end)
    end,
    "filter by text, tag or project",
  },
  ["<Tab>"] = {
    function(_, ctx)
      if ctx.project then
        ctx.project, ctx.title = nil, nil
      else
        local p = current_project()
        if not p then
          vim.notify("Den: this folder is not a project")
          return
        end
        ctx.project, ctx.title = p.name, p.title
      end
      S.render(NAME)
    end,
    "this project ↔ all projects",
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

--- Opens the Tasks screen for the current folder's project, or everything.
function M.open(opts)
  opts = opts or {}
  S.open(NAME, { render = render, keys = keys, ctx = {} })
  local ctx = S.ctx(NAME)
  local function set_scope()
    if opts.all then
      ctx.project, ctx.title = nil, nil
    elseif opts.project then
      local p = native.call("project", opts.project)
      ctx.project, ctx.title = opts.project, p and p.title or opts.project
    else
      local p = current_project()
      ctx.project, ctx.title = p and p.name or nil, p and p.title or nil
    end
    S.render(NAME)
  end
  if state.ready then
    set_scope()
  else
    state.when_ready(set_scope)
  end
end

M.render = render
M.keys = keys

return M
