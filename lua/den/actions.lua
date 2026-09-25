-- What a key does to a task, shared by every screen. Each action plans with
-- the engine, applies the change, and keeps the timer in step.

local native = require("den.native")
local apply = require("den.apply")
local state = require("den.state")
local util = require("den.util")

local M = {}

local function warn(msg)
  vim.notify("Den: " .. msg, vim.log.levels.WARN)
end

--- Whether the timer is running on this task.
function M.is_timed(row)
  local r = native.call("timer_running")
  return r ~= nil and r.file == row.path and r.task == row.title
end

--- Marks a task doing and starts timing it (stopping any other timer).
function M.start(row)
  if not row then
    return
  end
  if row.state ~= "doing" then
    local ok = apply.run(function(mod)
      return mod.plan_state(util.ref(row), "doing")
    end)
    if not ok then
      return
    end
  end
  local _, err = native.call("timer_start", row.path, row.title)
  if err then
    warn(err)
  end
  state.changed()
end

--- Stops the timer. The task stays doing.
function M.pause()
  if native.call("timer_running") then
    native.call("timer_stop")
    state.changed()
  end
end

local function set(row, target)
  local timed = M.is_timed(row)
  local ok = apply.run(function(mod)
    return mod.plan_state(util.ref(row), target)
  end)
  if ok and timed and target ~= "doing" then
    native.call("timer_stop")
  end
  state.changed()
  return ok
end

function M.done(row)
  if row then
    set(row, row.state == "done" and "open" or "done")
  end
end

function M.drop(row)
  if row then
    set(row, row.state == "dropped" and "open" or "dropped")
  end
end

function M.reopen(row)
  if row then
    set(row, "open")
  end
end

--- Opens the task's file at its line.
function M.open(row)
  if not row then
    return
  end
  local abs = native.call("abs", row.path)
  if not abs then
    return
  end
  vim.cmd.edit(vim.fn.fnameescape(abs))
  pcall(vim.api.nvim_win_set_cursor, 0, { row.line + 1, 0 })
end

--- The task at `row.line` of its file now, after an edit.
local function reread(path, line)
  for _, t in ipairs(native.call("doc_tasks", path) or {}) do
    if t.line == line then
      return t
    end
  end
end

--- Rewrites a task's text; its timer history follows the new title.
function M.edit(row)
  if not row then
    return
  end
  vim.ui.input({ prompt = "Task: ", default = row.text }, function(text)
    if not text or vim.trim(text) == "" or text == row.text then
      return
    end
    local ok = apply.run(function(mod)
      return mod.plan_edit(util.ref(row), text)
    end)
    if ok then
      local now = reread(row.path, row.line)
      if now and now.title ~= row.title then
        native.call("timer_rename", row.path, row.title, row.path, now.title)
      end
      state.changed()
    end
  end)
end

--- Moves a task into a section of a project (`nil` = its own project).
function M.move(row, project, section)
  if not row then
    return
  end
  local ok = apply.run(function(mod)
    return mod.plan_move(util.ref(row), project, section or "next_actions")
  end)
  if not ok then
    return
  end
  -- With no project named, a task in a project's note moves into the
  -- project's own file.
  local name = project or (row.project and row.project.name)
  local target = row.path
  if name then
    local p = native.call("project", name)
    target = p and p.path or target
  end
  if target ~= row.path then
    native.call("timer_rename", row.path, row.title, target, row.title)
  end
  state.changed()
end

--- Moves a task to another project, chosen in a picker.
function M.move_pick(row)
  if not row then
    return
  end
  local current = row.project and row.project.name
  require("den.pick").project({ prompt = "Move to", exclude = current }, function(p)
    M.move(row, p.name, "next_actions")
  end)
end

--- Asks for a task and adds it to a project's section.
function M.add(project, section)
  local function ask(name)
    vim.ui.input({ prompt = "New task: " }, function(text)
      if text and vim.trim(text) ~= "" then
        apply.run(function(mod)
          return mod.plan_add(name, text, section or "next_actions")
        end)
      end
    end)
  end
  if project then
    ask(project)
  else
    require("den.pick").project({ prompt = "Add to" }, function(p)
      ask(p.name)
    end)
  end
end

return M
