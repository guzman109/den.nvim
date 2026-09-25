-- The Review screen, focus rings, journal facts, charts and break nudges.
-- Headless, so every chart is drawn with block characters.

local native = require("den.native")
local charts = require("den.charts")
local statusline = require("den.statusline")

T.test("block charts scale to their space", function()
  T.eq(charts.hbar(0, 10, 8), "")
  T.eq(charts.hbar(10, 10, 4), "████")
  T.eq(charts.hbar(5, 10, 4), "██")
  T.eq(charts.hbar(1, 1000, 4), "▏", "a small value still shows")
  T.eq(charts.meter(2, 7, 7), "▰▰▱▱▱▱▱")
  T.eq(charts.columns({ 0, 1, 2 }, 2), { "  █", "·██" })
  local b = charts.burndown({ { count = 4 }, { count = 4 }, { count = 2 } }, 5, 4, 10)
  T.eq(#b, 5, "four rows and an axis")
  T.eq(b[5], "──────────")
  T.ok(b[1]:find("██", 1, true), b[1])
end)

T.test("the review shows finished work, time, progress and a burndown", function()
  vim.cmd.cd(T.tmp)
  require("den.screens.review").open({ all = true })
  T.settle()
  local lines = T.lines()
  T.contains(lines, "Review · all projects")
  T.contains(lines, "Finished · last 14 days")
  T.contains(lines, "Time this week")
  T.contains(lines, "Progress")
  T.contains(lines, "Personal website")
  T.contains(lines, "2 of 7 done")
  T.contains(lines, "Burndown · Personal website")
  T.contains(lines, "1 Sep")
  T.contains(lines, "1 Oct")
end)

T.test("enter on a project opens its tasks", function()
  require("den.screens.review").open({ all = true })
  T.settle()
  for row, line in ipairs(T.lines()) do
    if line:find("Personal website", 1, true) and line:find("done", 1, true) then
      vim.api.nvim_win_set_cursor(0, { row, 0 })
      break
    end
  end
  vim.cmd("normal \r")
  T.eq(vim.api.nvim_buf_get_name(0), "den://tasks")
  T.contains(T.lines(), "Tasks · Personal website")
end)

T.test("the focus window shows the session and today without taking the cursor", function()
  local before = vim.api.nvim_get_current_win()
  native.call("timer_stop")
  native.call("timer_start", "projects/website.md", "Draft the homepage story")
  local focus = require("den.focus")
  focus.open()
  T.ok(focus.is_open())
  T.eq(vim.api.nvim_get_current_win(), before)
  local f = native.call("focus")
  local text = {}
  for _, parts in ipairs(focus.lines(f)) do
    local s = ""
    for _, p in ipairs(parts) do
      s = s .. p[1]
    end
    table.insert(text, s)
  end
  T.contains(text, "◐ Draft the homepage story")
  T.contains(text, "session  00:0")
  T.contains(text, "/ 50m")
  T.contains(text, "/ 4h")
  T.contains(text, "steps    no data yet")
  focus.toggle()
  T.ok(not focus.is_open())
  native.call("timer_stop")
end)

T.test("the focus rings open full size in a tab of their own", function()
  local tabs = #vim.api.nvim_list_tabpages()
  vim.cmd("Den focus tab")
  T.eq(#vim.api.nvim_list_tabpages(), tabs + 1)
  T.eq(vim.api.nvim_buf_get_name(0), "den://focus")
  local text = table.concat(T.lines(), "\n")
  T.contains(text, "session  ")
  T.contains(text, "today    ")
  -- Centred: the words do not start at the left edge.
  for _, line in ipairs(T.lines()) do
    if line:find("session", 1, true) then
      T.ok(line:match("^%s+session"), line)
    end
  end
  T.ok(require("den.focus").tab_open())
  vim.cmd("normal q")
  T.eq(#vim.api.nvim_list_tabpages(), tabs)
  T.ok(not require("den.focus").tab_open())
end)

T.test(":tab Den review opens the review in a new tab", function()
  local tabs = #vim.api.nvim_list_tabpages()
  vim.cmd("tab Den review all")
  T.eq(#vim.api.nvim_list_tabpages(), tabs + 1)
  T.eq(vim.api.nvim_buf_get_name(0), "den://review")
  vim.cmd("tabclose")
end)

T.test("a journal page shows the day's facts without changing its text", function()
  vim.cmd.edit(T.vault .. "/daily/2026-09-24.md")
  local before = table.concat(T.lines(), "\n")
  require("den.decor").decorate(0)
  local marks = vim.api.nvim_buf_get_extmarks(0, -1, 0, -1, { details = true })
  local facts = {}
  for _, m in ipairs(marks) do
    for _, vl in ipairs(m[4].virt_lines or {}) do
      local s = ""
      for _, chunk in ipairs(vl) do
        s = s .. chunk[1]
      end
      table.insert(facts, s)
    end
  end
  -- The fixture log has a session still running, so the total depends on
  -- the clock; the rest is fixed.
  T.contains(facts, "worked  ")
  T.contains(facts, "Wire up the renderer")
  T.contains(facts, "finished 2")
  T.contains(facts, "Sketch the lua_module surface")
  T.contains(facts, "walked  25m")
  T.eq(table.concat(T.lines(), "\n"), before)
  T.ok(not vim.bo.modified)
end)

T.test("late in the day an unwritten journal is mentioned once", function()
  local journal = require("den.journal")
  T.ok(journal.has_writing("---\ndate: x\n---\n# Day\n\n## On my mind\n\nA walk by the river.\n"))
  T.ok(not journal.has_writing("---\ndate: x\nmood:\n---\n# Day\n\n## On my mind\n\n## Went well\n"))
  local hour = journal.LATE_HOUR
  journal.LATE_HOUR = -1
  journal.refresh("2026-09-20")
  statusline.refresh()
  -- The quietest insight: it only shows when fewer urgent things compete.
  T.lacks(statusline.text(2), "journal")
  T.contains(statusline.text(10), "journal · not written today")
  journal.LATE_HOUR = 25
  statusline.refresh()
  T.lacks(statusline.text(10), "journal")
  journal.LATE_HOUR = -1
  journal.refresh("2026-09-24")
  statusline.refresh()
  T.lacks(statusline.text(10), "journal", "that page has writing")
  journal.LATE_HOUR = hour
end)

T.test("a long stretch brings a nudge that does not take the cursor", function()
  local nudges = require("den.nudges")
  local before = vim.api.nvim_get_current_win()
  nudges._sit(20)
  T.eq(nudges.check(), nil, "too soon")
  nudges._sit(95)
  local n = nudges.check()
  T.ok(n, "a nudge")
  T.eq(n.reason, "chair")
  T.eq(vim.api.nvim_get_current_win(), before)
  local s = nudges._state()
  T.ok(s.win and vim.api.nvim_win_is_valid(s.win), "the window shows")
  T.contains(vim.api.nvim_buf_get_lines(vim.api.nvim_win_get_buf(s.win), 0, -1, false), "w walk · z snooze 30 min · q not today")
  statusline.refresh()
  T.contains(statusline.text(1), "break · 1h 35m in the chair")

  -- :Den break steps in; z snoozes.
  vim.cmd("Den break")
  T.eq(vim.api.nvim_get_current_win(), s.win)
  vim.cmd("normal z")
  T.contains(T.notes[#T.notes], "snoozed for 30 minutes")
  T.eq(nudges.check(), nil, "snoozed")
end)

T.test("a walk counts as a break and is written to the timer log", function()
  local nudges = require("den.nudges")
  local keys = nudges.keys
  keys.w()
  T.ok(nudges._state().walking)
  T.contains(T.read(".den/log/test.jsonl"), '"event":"walk_start"')
  -- Coming back (any key) ends the walk.
  vim.api.nvim_feedkeys("j", "x", false)
  T.settle(1100)
  vim.api.nvim_feedkeys("k", "x", false)
  T.settle()
  T.ok(not nudges._state().walking)
  T.contains(T.read(".den/log/test.jsonl"), '"event":"walk_end"')
end)

T.test("turning nudges off asks three times, and saying no keeps them", function()
  local nudges = require("den.nudges")
  local asked = {}
  local real = vim.ui.select
  local answers = { "Yes, turn them off", "Yes, really", "Keep them after all" }
  vim.ui.select = function(items, opts, cb)
    table.insert(asked, opts.prompt)
    cb(answers[#asked])
  end
  nudges.off()
  vim.ui.select = real
  T.eq(#asked, 3)
  T.eq(asked[1], "Turn off break reminders? Your legs were counting on you.")
  T.contains(T.notes[#T.notes], "The reminders stay")
  T.eq(nudges.describe(), "on")
end)

T.test("nothing in setup or the config turns nudges off", function()
  local nudges_src = io.open(T.root .. "/lua/den/nudges.lua"):read("*a")
  T.contains(nudges_src, "TURNING NUDGES OFF IS FOR HUMANS ONLY")
  local ok, err = pcall(require("den").setup, { vault = T.vault, machine = "test", nudges = false, ask_about_folders = false })
  T.ok(ok, tostring(err))
  require("den.state").wait(5000)
  T.eq(require("den.nudges").describe(), "on")
end)
