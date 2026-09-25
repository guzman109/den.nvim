-- Pickers. fzf-lua (with whatever finder it runs, such as skim) when it is
-- installed, else Neovim's own vim.ui.select.

local native = require("den.native")

local M = {}

--- Shows `items` (tables with a `label`) and calls `on_choice(item)`.
function M.select(items, opts, on_choice)
  if #items == 0 then
    vim.notify("Den: nothing to pick from")
    return
  end
  local has_fzf, fzf = pcall(require, "fzf-lua")
  if has_fzf and not opts.plain then
    local by_label, labels = {}, {}
    for _, item in ipairs(items) do
      local label = item.label
      while by_label[label] do
        label = label .. " "
      end
      by_label[label] = item
      table.insert(labels, label)
    end
    fzf.fzf_exec(labels, {
      prompt = (opts.prompt or "Den") .. "> ",
      actions = {
        ["default"] = function(selected)
          local item = selected and by_label[selected[1]]
          if item then
            on_choice(item)
          end
        end,
      },
    })
    return
  end
  vim.ui.select(items, {
    prompt = opts.prompt,
    format_item = function(item)
      return item.label
    end,
  }, function(item)
    if item then
      on_choice(item)
    end
  end)
end

--- Picks an active project.
function M.project(opts, on_choice)
  local items = {}
  for _, p in ipairs(native.call("projects") or {}) do
    if p.name ~= opts.exclude and p.status ~= "archived" and not p.locked then
      table.insert(items, { label = p.title, name = p.name, project = p })
    end
  end
  table.sort(items, function(a, b)
    return a.label:lower() < b.label:lower()
  end)
  M.select(items, opts, on_choice)
end

local function open_path(rel, line)
  local abs = native.call("abs", rel)
  if abs then
    vim.cmd.edit(vim.fn.fnameescape(abs))
    if line then
      pcall(vim.api.nvim_win_set_cursor, 0, { line + 1, 0 })
    end
  end
end

--- Finds any project, note or journal page by name.
function M.find()
  local items = {}
  for _, d in ipairs(native.call("docs") or {}) do
    if d.kind ~= "template" and not d.locked then
      table.insert(items, { label = d.kind .. " · " .. d.title, path = d.path })
    end
  end
  table.sort(items, function(a, b)
    return a.label:lower() < b.label:lower()
  end)
  M.select(items, { prompt = "Find" }, function(item)
    open_path(item.path)
  end)
end

--- Notes that belong to a project (the current one when `name` is nil).
function M.notes(name)
  local p = name and native.call("project", name)
  if not p then
    vim.notify("Den: not in a project")
    return
  end
  local items = { { label = "project · " .. p.title, path = p.path } }
  for _, n in ipairs(p.notes) do
    table.insert(items, { label = n.title, path = n.path })
  end
  M.select(items, { prompt = p.title .. " notes" }, function(item)
    open_path(item.path)
  end)
end

--- Every open task, to jump to.
function M.tasks()
  local view = native.call("tasks_view") or { doing = {}, groups = {} }
  local items = {}
  local function add(row)
    local where = row.project and row.project.title or row.source
    table.insert(items, { label = row.title .. "  · " .. where, row = row })
  end
  for _, row in ipairs(view.doing) do
    add(row)
  end
  for _, g in ipairs(view.groups) do
    for _, row in ipairs(g.tasks) do
      add(row)
    end
  end
  M.select(items, { prompt = "Tasks" }, function(item)
    open_path(item.row.path, item.row.line)
  end)
end

--- Searches inside notes, limited to the vault.
function M.grep()
  local root = native.call("root")
  local has_fzf, fzf = pcall(require, "fzf-lua")
  if has_fzf then
    fzf.live_grep({ cwd = root, prompt = "Den> " })
    return
  end
  vim.ui.input({ prompt = "Search notes: " }, function(query)
    if query and query ~= "" then
      vim.cmd("silent grep! " .. vim.fn.shellescape(query) .. " " .. vim.fn.fnameescape(root))
      vim.cmd("copen")
    end
  end)
end

return M
