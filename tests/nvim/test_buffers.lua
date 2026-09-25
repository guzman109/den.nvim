local native = require("den.native")
local actions = require("den.actions")

local function row(project, title)
  local view = native.call("tasks_view", project)
  for _, r in ipairs(view.doing) do
    if r.title == title then
      return r
    end
  end
  for _, g in ipairs(view.groups) do
    for _, r in ipairs(g.tasks) do
      if r.title == title then
        return r
      end
    end
  end
  error("no open task " .. title)
end

T.test("a change to a file with unsaved edits goes into the buffer, not the disk", function()
  vim.cmd.edit(T.vault .. "/projects/website.md")
  local buf = vim.api.nvim_get_current_buf()
  local lines = T.lines(buf)
  for i, l in ipairs(lines) do
    if l:find("A small site", 1, true) then
      vim.api.nvim_buf_set_lines(buf, i - 1, i, false, { l .. " EDITED" })
    end
  end
  T.ok(vim.bo[buf].modified, "the buffer has unsaved edits")

  actions.done(row("website", "Draft the homepage story"))

  local text = table.concat(T.lines(buf), "\n")
  T.contains(text, "- [x] Draft the homepage story #writing @due(2026-09-28) @done(")
  T.contains(text, "EDITED", "the unsaved edit survives")
  T.ok(vim.bo[buf].modified, "still unsaved; Den never saves someone's edits for them")
  local disk = T.read("projects/website.md")
  T.lacks(disk, "EDITED")
  T.contains(disk, "- [/] Draft the homepage story", "the disk is untouched until the person saves")

  vim.cmd("silent write")
  T.contains(T.read("projects/website.md"), "- [x] Draft the homepage story")
end)

T.test("a change to an open file without unsaved edits is written straight away", function()
  vim.cmd.edit(T.vault .. "/projects/haste.md")
  local buf = vim.api.nvim_get_current_buf()
  actions.drop(row("haste", "Profile the first frame"))
  T.ok(not vim.bo[buf].modified, "written, not left unsaved")
  T.contains(T.lines(buf), "- [-] Profile the first frame #rust @due(2026-09-24)")
  T.contains(T.read("projects/haste.md"), "- [-] Profile the first frame #rust @due(2026-09-24)")
end)

T.test("due dates and a project's notes are drawn without changing the text", function()
  vim.cmd.edit(T.vault .. "/projects/website.md")
  local buf = vim.api.nvim_get_current_buf()
  local before = T.lines(buf)
  require("den.decor").decorate(buf)
  local ns = vim.api.nvim_get_namespaces()["den.decor"]
  local marks = vim.api.nvim_buf_get_extmarks(buf, ns, 0, -1, { details = true })
  local virt, notes = {}, nil
  for _, m in ipairs(marks) do
    for _, chunk in ipairs(m[4].virt_text or {}) do
      table.insert(virt, chunk[1])
    end
    for _, vl in ipairs(m[4].virt_lines or {}) do
      local parts = {}
      for _, chunk in ipairs(vl) do
        table.insert(parts, chunk[1])
      end
      notes = table.concat(parts)
    end
  end
  T.ok(#virt > 0, "a due date gets its distance in words")
  T.contains(notes or "", "Homepage story", "linked notes are listed under the title")
  T.eq(T.lines(buf), before, "the text is unchanged")
end)

T.test("today's journal page opens, created from the template when new", function()
  require("den.journal").open("2026-10-02")
  T.contains(vim.api.nvim_buf_get_name(0), "daily/2026-10-02.md")
  local lines = T.lines()
  T.eq(lines[5], "# Friday 2 October")
  T.contains(lines, "## On my mind")
end)

T.test("an unknown code folder is asked about once, and c creates its project", function()
  local dir = T.tmp .. "/code/kiln"
  vim.fn.mkdir(dir .. "/.git", "p")
  local folder = require("den.folder")
  local top = folder.candidate(dir)
  T.eq(top, dir)
  local win = folder.ask(top)
  vim.api.nvim_set_current_win(win)
  vim.api.nvim_feedkeys("c", "x", false)
  T.contains(T.read("projects/kiln.md"), "# kiln")
  T.eq(folder.candidate(dir), nil, "now a project")
  local p = native.call("project_for_dir", dir)
  T.eq(p and p.name, "kiln")
end)

T.test("a change made outside Den reaches the screens", function()
  require("den.screens.tasks").open({ project = "haste" })
  local path = T.vault .. "/projects/haste.md"
  local f = assert(io.open(path, "a"))
  f:write("- [ ] Written by another editor\n")
  f:close()
  local seen = vim.wait(5000, function()
    return table.concat(T.lines(), "\n"):find("Written by another editor", 1, true) ~= nil
  end, 20)
  T.ok(seen, "the watcher reloads the file and the screen redraws")
end)

T.test("other writers keep off a file with unsaved changes", function()
  local path = T.vault .. "/projects/haste.md"
  vim.cmd.edit(path)
  vim.api.nvim_buf_set_lines(0, -1, -1, false, { "- [ ] Typed, not saved" })
  T.settle()
  local den = require("den.sync").program()
  T.ok(den, "bin/den is built")
  local res = vim.system({ den, "capture", "--vault", T.vault, "--to", "haste", "From the shell" }, { text = true }):wait()
  T.ok(res.code ~= 0, "refused: " .. (res.stdout or ""))
  T.contains(res.stderr, "unsaved changes in Neovim")
  T.lacks(T.read("projects/haste.md"), "From the shell")
  -- Saved, the file is fair game again.
  vim.cmd("silent write")
  T.settle()
  res = vim.system({ den, "capture", "--vault", T.vault, "--to", "haste", "From the shell" }, { text = true }):wait()
  T.eq(res.code, 0, res.stderr)
  T.contains(T.read("projects/haste.md"), "From the shell")
end)

local function append(rel, text)
  local f = assert(io.open(T.vault .. "/" .. rel, "a"))
  f:write(text)
  f:close()
end

T.test("a move across two files is applied whole or not at all", function()
  vim.cmd.edit(T.vault .. "/projects/website.md")
  local buf = vim.api.nvim_get_current_buf()
  T.settle()
  local task = row("website", "Choose three projects to feature")
  -- The target changes on disk before Den hears about it.
  append("projects/haste.md", "- [ ] Changed behind Den's back\n")
  actions.move(task, "haste")
  T.contains(T.lines(buf), "- [ ] Choose three projects to feature", "the task stays where it was")
  T.contains(T.read("projects/website.md"), "- [ ] Choose three projects to feature")
  T.lacks(T.read("projects/haste.md"), "Choose three projects to feature")
  T.settle()
end)

T.test("an open file changed on disk behind Den's back is not overwritten", function()
  vim.cmd.edit(T.vault .. "/projects/haste.md")
  local buf = vim.api.nvim_get_current_buf()
  T.settle()
  local task = row("haste", "Wire up the renderer")
  append("projects/haste.md", "- [ ] Also written elsewhere\n")
  actions.drop(task)
  T.contains(T.read("projects/haste.md"), "Also written elsewhere", "the outside change survives")
  T.contains(T.read("projects/haste.md"), "- [/] Wire up the renderer")
  T.lacks(T.lines(buf), "- [-] Wire up the renderer")
  T.settle()
end)

T.test("Den changes a file's own buffer, never one whose name only contains it", function()
  local copy = T.vault .. "/projects/website.md.orig"
  vim.fn.writefile(vim.fn.readfile(T.vault .. "/projects/website.md"), copy)
  vim.cmd.edit(copy)
  local buf = vim.api.nvim_get_current_buf()
  local before = T.lines(buf)
  actions.drop(row("website", "Gather a few visual references"))
  T.eq(T.lines(buf), before, "the other buffer is untouched")
  T.contains(T.read("projects/website.md"), "- [-] Gather a few visual references")
end)

T.test("moving a task out of a project's note carries its timer", function()
  local task = row("website", "Ask two friends to read the draft")
  T.eq(task.path, "notes/homepage-story.md")
  actions.start(task)
  actions.move(row("website", "Ask two friends to read the draft"))
  T.contains(T.read("projects/website.md"), "Ask two friends to read the draft")
  local running = native.call("timer_running")
  T.eq(running and running.file, "projects/website.md", "the timer follows the task")
  T.ok(actions.is_timed(row("website", "Ask two friends to read the draft")), "x can stop it")
  actions.pause()
end)

T.test("engine errors read without the Lua stack traceback", function()
  T.eq(native.message("runtime error: projects/x.md changed\nstack traceback:\n\t[C]: in ?"), "projects/x.md changed")
  local _, err = native.call("plan_move", { path = "projects/nope.md", line = 0, raw = "x" }, nil, "next_actions")
  T.ok(err and not err:find("traceback", 1, true), tostring(err))
end)

T.test("a vault file opened while the vault loads is known as one", function()
  local state = require("den.state")
  require("den").setup({ vault = T.vault, machine = "test", ask_about_folders = false })
  T.ok(not state.ready, "still loading")
  T.eq(native.call("rel", T.vault .. "/notes/private.md.age"), "notes/private.md.age")
  T.ok(state.wait(10000), "the vault loads")
end)
