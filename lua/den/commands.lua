-- `:Den <subcommand>`.

local M = {}

local function den()
  return require("den")
end

local function needs_vault(fn)
  return function(args)
    local state = require("den.state")
    if not den().ensure() then
      return
    end
    if state.ready then
      fn(args)
    else
      vim.notify("Den: loading your vault…")
      state.when_ready(function()
        fn(args)
      end)
    end
  end
end

M.subcommands = {
  tasks = needs_vault(function(args)
    require("den.screens.tasks").open({ all = args[1] == "all", project = args[1] ~= "all" and args[1] or nil })
  end),
  inbox = needs_vault(function(args)
    require("den.screens.inbox").open({ project = args[1] })
  end),
  capture = needs_vault(function(args)
    if #args > 0 then
      require("den.capture").save(table.concat(args, " "))
    else
      require("den.capture").open()
    end
  end),
  project = needs_vault(function(args)
    local folder = require("den.folder")
    local top = vim.fs.root(vim.fn.getcwd(), ".git") or vim.fn.getcwd()
    if args[1] == "new" then
      local title = table.concat(vim.list_slice(args, 2), " ")
      local apply = require("den.apply")
      local function create(t)
        if t and vim.trim(t) ~= "" and apply.run(function(mod)
          return mod.plan_new_project(t, top)
        end) then
          vim.notify("Den: created project " .. t)
        end
      end
      if title ~= "" then
        create(title)
      else
        vim.ui.input({ prompt = "Project title: ", default = vim.fn.fnamemodify(top, ":t") }, create)
      end
    else
      folder.ask(top)
    end
  end),
  notes = needs_vault(function(args)
    local name = args[1]
    if not name then
      local p = require("den.native").call("project_for_dir", vim.fn.getcwd())
      name = p and p.name
    end
    require("den.pick").notes(name)
  end),
  find = needs_vault(function()
    require("den.pick").find()
  end),
  search = needs_vault(function()
    require("den.pick").grep()
  end),
  today = needs_vault(function(args)
    require("den.journal").open(args[1])
  end),
  timer = needs_vault(function(args)
    local native = require("den.native")
    local util = require("den.util")
    if args[1] == "stop" or args[1] == "pause" then
      require("den.actions").pause()
    else
      local r = native.call("timer_running")
      if r then
        vim.notify(("Den: %s · %s"):format(r.task, util.clock(r.seconds)))
      else
        vim.notify("Den: no timer running")
      end
    end
  end),
  sync = needs_vault(function(args)
    require("den.sync").command(args)
  end),
  build = function()
    require("den.native").build()
  end,
  health = function()
    vim.cmd("checkhealth den")
  end,
}

--- Runs `:Den [sub] [args…]`. With no subcommand, opens Tasks.
function M.run(cmd)
  local args = vim.deepcopy(cmd.fargs)
  local sub = table.remove(args, 1) or "tasks"
  local fn = M.subcommands[sub]
  if not fn then
    vim.notify("Den: no subcommand " .. sub, vim.log.levels.WARN)
    return
  end
  fn(args)
end

function M.complete(arglead, cmdline)
  local words = vim.split(cmdline, "%s+", { trimempty = true })
  if #words <= 1 or (#words == 2 and not cmdline:match("%s$")) then
    local names = vim.tbl_keys(M.subcommands)
    table.sort(names)
    return vim.tbl_filter(function(n)
      return vim.startswith(n, arglead)
    end, names)
  end
  if words[2] == "tasks" then
    return vim.tbl_filter(function(n)
      return vim.startswith(n, arglead)
    end, { "all" })
  end
  if words[2] == "project" then
    return { "new" }
  end
  if words[2] == "sync" then
    return { "continue" }
  end
  return {}
end

return M
