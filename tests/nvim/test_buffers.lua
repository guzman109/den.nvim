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
