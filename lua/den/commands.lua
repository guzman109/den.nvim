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
  review = needs_vault(function(args)
    if vim.tbl_contains(args, "tab") and not args.tab then
      vim.cmd("tabnew")
    end
    require("den.screens.review").open({ all = vim.tbl_contains(args, "all") })
  end),
  focus = needs_vault(function(args)
    local focus = require("den.focus")
    if args[1] == "tab" or args.tab then
      focus.open_tab()
    else
      focus.toggle()
    end
  end),
  ["break"] = needs_vault(function()
    require("den.nudges").enter()
  end),
  nudges = needs_vault(function(args)
    local nudges = require("den.nudges")
    if args[1] == "off" then
      nudges.off()
    elseif args[1] == "on" then
      nudges.on()
    else
      vim.notify("Den: break reminders are " .. nudges.describe())
    end
  end),
  lock = needs_vault(function(args)
    local locked = require("den.locked")
    local what = args[1]
    if not what then
      locked.forget()
    elseif what == "note" then
      locked.lock_note()
    elseif what == "setup" then
      locked.setup_keys()
    elseif what == "password" then
      locked.change_password()
    elseif what == "touch-id" then
      locked.enable_touch_id()
    elseif what == "status" then
      local s, why = locked.status()
      if not s then
        vim.notify("Den: " .. tostring(why), vim.log.levels.WARN)
      elseif not s.set_up then
        vim.notify("Den: locking is not set up (:Den lock setup)")
      else
        vim.notify(("Den: vault %s · unlocks with %s%s"):format(
          s.unlocked and "unlocked" or "locked",
          table.concat(s.methods, ", "),
          s.strict and " · strict" or ""
        ))
      end
    else
      vim.notify("Den: :Den lock [note|setup|password|touch-id|status]", vim.log.levels.WARN)
    end
  end),
  unlock = needs_vault(function(args)
    local locked = require("den.locked")
    if args[1] == "note" then
      locked.unlock_note()
    else
      local method = ({ recovery = "recovery", yubikey = "yubikey", ["touch-id"] = "touch_id", password = "password" })[args[1] or ""]
      locked.unlock(nil, method)
    end
  end),
  sync = needs_vault(function(args)
    require("den.sync").command(args)
  end),
  build = function(args)
    require("den.native").build(function(ok)
      if ok and not require("den.native").loaded() then
        den().setup(nil, true)
      end
    end, args[1])
  end,
  health = function()
    vim.cmd("checkhealth den")
  end,
}

--- Runs `:Den [sub] [args…]`. With no subcommand, opens Tasks.
--- Subcommands that open a screen in the current window; `:tab Den …`
--- gives them a new tab first.
local SCREENS = { tasks = true, inbox = true, review = true, today = true, sync = true }

function M.run(cmd)
  local args = vim.deepcopy(cmd.fargs)
  local sub = table.remove(args, 1) or "tasks"
  local fn = M.subcommands[sub]
  if not fn then
    vim.notify("Den: no subcommand " .. sub, vim.log.levels.WARN)
    return
  end
  -- `:tab Den focus`, `:tab Den review` and so on.
  if cmd.smods and cmd.smods.tab and cmd.smods.tab >= 0 then
    args.tab = true
    if SCREENS[sub] then
      vim.cmd("tabnew")
    end
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
  if words[2] == "nudges" then
    return vim.tbl_filter(function(n)
      return vim.startswith(n, arglead)
    end, { "on", "off" })
  end
  if words[2] == "tasks" then
    return vim.tbl_filter(function(n)
      return vim.startswith(n, arglead)
    end, { "all" })
  end
  if words[2] == "review" then
    return vim.tbl_filter(function(n)
      return vim.startswith(n, arglead)
    end, { "all", "tab" })
  end
  if words[2] == "focus" then
    return vim.tbl_filter(function(n)
      return vim.startswith(n, arglead)
    end, { "tab" })
  end
  if words[2] == "build" then
    return { "download" }
  end
  if words[2] == "project" then
    return { "new" }
  end
  if words[2] == "sync" then
    return { "continue" }
  end
  if words[2] == "lock" then
    return vim.tbl_filter(function(n)
      return vim.startswith(n, arglead)
    end, { "note", "setup", "password", "touch-id", "status" })
  end
  if words[2] == "unlock" then
    return vim.tbl_filter(function(n)
      return vim.startswith(n, arglead)
    end, { "note", "password", "recovery", "yubikey", "touch-id" })
  end
  return {}
end

return M
