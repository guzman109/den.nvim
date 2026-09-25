-- The Review screen: what got finished, where the time went, and how each
-- project is doing against its end date.
--
-- In kitty the charts are real images; everywhere else they are drawn with
-- block characters. Both come from the same numbers.

local S = require("den.ui.screen")
local rows = require("den.ui.rows")
local charts = require("den.charts")
local native = require("den.native")
local state = require("den.state")
local util = require("den.util")

local M = {}

local NAME = "review"
local DAYS = { "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun" }

--- Spreads a row of one-cell bars out: each bar two cells, a gap after.
local function spread(row)
  local out = {}
  for _, ch in ipairs(vim.fn.split(row, "\\zs")) do
    table.insert(out, ch .. ch .. " ")
  end
  return table.concat(out)
end

local function section(title, right, width)
  local l = S.line()
  S.add(l, "  ")
  S.add(l, title, "DenGroup")
  if right and right ~= "" then
    local gap = width - 2 - util.width(title) - util.width(right) - 2
    S.add(l, string.rep(" ", math.max(2, gap)))
    S.add(l, right, "DenMuted")
  end
  return l
end

local function images()
  local ok, kitty = pcall(require, "den.kitty")
  return ok and kitty.enabled() and kitty or nil
end

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
  local r = native.need().review(ctx.project)
  local kitty = images()
  local title = ctx.project and ("Review · " .. (ctx.title or ctx.project)) or "Review · all projects"
  push(rows.header(title, "week of " .. util.short_date(r.week[1].date), width))

  -- Finished, per day.
  local counts, finished = {}, 0
  for _, d in ipairs(r.closed) do
    table.insert(counts, d.count)
    finished = finished + d.count
  end
  push(S.line())
  push(section("Finished · last 14 days", finished == 1 and "1 task" or (finished .. " tasks"), width))
  local placed = kitty and kitty.place(lines, "closed", r.closed, 44, 5)
  if not placed then
    for _, row in ipairs(charts.columns(counts, 4)) do
      push(S.add(S.line(), "    " .. spread(row), "DenChart"))
    end
    local first, last = util.short_date(r.closed[1].date), util.short_date(r.closed[#r.closed].date)
    local axis = #counts * 3 - 1
    push(S.add(S.line(), "    " .. first .. string.rep(" ", math.max(1, axis - util.width(first) - util.width(last))) .. last, "DenMuted"))
  end

  -- Time this week.
  push(S.line())
  push(section("Time this week", util.duration(r.week_total), width))
  local most = 0
  for _, d in ipairs(r.week) do
    most = math.max(most, d.seconds)
  end
  local bar = math.max(10, math.min(40, width - 24))
  for i, d in ipairs(r.week) do
    local l = S.line()
    local future = d.date > r.today
    S.add(l, "    " .. DAYS[i] .. "  ", d.date == r.today and "DenKey" or "DenMuted")
    if future then
      S.add(l, "")
    elseif d.seconds == 0 then
      S.add(l, "·", "DenMuted")
    else
      local b = charts.hbar(d.seconds, most, bar)
      S.add(l, b, "DenChart")
      S.add(l, string.rep(" ", bar - util.width(b) + 2) .. util.duration(d.seconds), "DenMuted")
    end
    push(l)
  end
  if #r.by_project > 1 or (#r.by_project == 1 and not ctx.project) then
    push(S.line())
    local label_w = 0
    for _, s in ipairs(r.by_project) do
      label_w = math.max(label_w, util.width(s.label))
    end
    label_w = math.min(label_w, 24)
    local top = r.by_project[1] and r.by_project[1].seconds or 0
    for _, s in ipairs(r.by_project) do
      local l = S.line()
      S.add(l, "    ")
      S.cell(l, s.label, label_w + 2, s.project and "DenProject" or "DenMuted")
      local room = math.max(8, bar - label_w - 3)
      local b = charts.hbar(s.seconds, top, room)
      S.add(l, b, "DenChart")
      S.add(l, string.rep(" ", room - util.width(b) + 2) .. util.duration(s.seconds), "DenMuted")
      push(l, s.project and { key = "p:" .. s.project.name, project = s.project.name } or nil)
    end
  end

  -- Progress per project.
  if #r.projects > 0 then
    push(S.line())
    push(section("Progress", nil, width))
    local label_w = 0
    for _, p in ipairs(r.projects) do
      label_w = math.max(label_w, util.width(p.project.title))
    end
    label_w = math.min(label_w, 24)
    for _, p in ipairs(r.projects) do
      local l = S.line()
      local total = p.done + p.open
      S.add(l, "    ")
      S.cell(l, p.project.title, label_w + 2, "DenProject")
      S.add(l, charts.meter(p.done, total, 10), "DenChart")
      local words = { ("%d of %d done"):format(p.done, total) }
      local hl = "DenMuted"
      if p.due then
        table.insert(words, "due " .. util.relative(p.due, r.today))
      end
      if p.pace and p.pace.behind then
        table.insert(words, "behind pace")
        hl = "DenDue"
      end
      S.add(l, "  " .. table.concat(words, " · "), hl)
      push(l, { key = "p:" .. p.project.name, project = p.project.name })
    end
  end

  -- Burndowns.
  for _, b in ipairs(r.burndowns) do
    push(S.line())
    local left = b.points[#b.points] and b.points[#b.points].count or 0
    push(section("Burndown · " .. b.project.title, ("%d left · due %s"):format(left, util.short_date(b.due)), width))
    local days = (util.days_between(b.start, b.due) or 0) + 1
    local shown = kitty and kitty.place(lines, "burndown", b, 60, 8)
    if not shown then
      local top = b.points[1] and b.points[1].count or 0
      for _, p in ipairs(b.points) do
        top = math.max(top, p.count)
      end
      local plot = math.max(20, math.min(days * 2, width - 12))
      local chart = charts.burndown(b.points, days, 6, plot)
      local label = tostring(top)
      for i, row in ipairs(chart) do
        local l = S.line()
        local axis = (i == 1 and label) or (i == #chart - 1 and "0") or ""
        S.add(l, string.rep(" ", 6 - #axis) .. axis .. " ", "DenMuted")
        S.add(l, row, i == #chart and "DenMuted" or "DenChart")
        push(l)
      end
      local from, to = util.short_date(b.start), util.short_date(b.due)
      local axis = util.width(chart[#chart])
      push(S.add(S.line(), "       " .. from .. string.rep(" ", math.max(1, axis - util.width(from) - util.width(to))) .. to, "DenMuted"))
    end
  end

  push(S.line())
  push(rows.hints({
    { "<CR>", "project tasks" },
    { "<Tab>", ctx.project and "all projects" or "this project" },
    { "g?", "keys" },
  }))
  return { lines = lines, items = items }
end

local keys
keys = {
  ["<CR>"] = {
    function(item)
      if item and item.project then
        require("den.screens.tasks").open({ project = item.project })
      end
    end,
    "open the project's tasks",
  },
  ["<Tab>"] = {
    function(_, ctx)
      if ctx.project then
        ctx.project, ctx.title = nil, nil
      else
        local p = native.call("project_for_dir", vim.fn.getcwd())
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
  r = {
    function()
      S.render(NAME)
    end,
    "redraw",
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

--- Opens the Review for this folder's project, or everything.
function M.open(opts)
  opts = opts or {}
  S.open(NAME, { render = render, keys = keys, ctx = {} })
  local ctx = S.ctx(NAME)
  local function set_scope()
    if opts.all then
      ctx.project, ctx.title = nil, nil
    else
      local p = native.call("project_for_dir", vim.fn.getcwd())
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
