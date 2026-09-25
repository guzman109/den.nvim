-- Den: projects, notes and next actions in a Markdown vault.
--
--   require("den").setup({
--     vault = "~/Notes/den",      -- optional; else ~/.config/den/config.yaml
--     ask_about_folders = true,   -- ask once about unknown code folders
--     images = true,              -- charts and focus rings as images in kitty
--     build = true,               -- build the engine when it is missing or old
--   })
--
-- Break nudges have no option here, on purpose: see lua/den/nudges.lua.
--
-- Den adds no key mappings. Everything is under :Den, and the Lua API below
-- is there to map as you like.

local M = {}

M.options = {}
local started = false

--- Starts Den. The first time (and after an update), the engine is built
--- from source before it is loaded, in the background; Den starts once the
--- build is done, with no restart. Returns false until it has started.
function M.setup(opts, built)
  M.options = vim.tbl_extend("force", M.options, opts or {})
  require("den.highlights").setup()
  local native = require("den.native")
  if native.building() then
    vim.notify("Den: the engine is still building")
    return false
  end
  if not built and not native.loaded() and M.options.build ~= false and native.stale() then
    native.build(function()
      -- Built or not: an older engine still loads, and a missing one says
      -- how to build it.
      if not started then
        M.setup(nil, true)
      end
    end)
    return false
  end
  local ok, err = pcall(require("den.state").setup, {
    vault = M.options.vault,
    machine = M.options.machine,
    config = M.options.config,
  })
  if not ok then
    vim.notify("Den: " .. native.message(err), vim.log.levels.ERROR)
    return false
  end
  started = true
  require("den.autocmds").setup(M.options)
  require("den.statusline").setup()
  require("den.sync").setup()
  require("den.journal").setup()
  require("den.nudges").setup()
  require("den.locked").setup()
  return true
end

--- Starts Den with the options given so far, if it hasn't started.
function M.ensure()
  if started then
    return true
  end
  return M.setup(M.options)
end

--- Opens the Tasks screen. `{ all = true }` for every project.
function M.tasks(opts)
  if M.ensure() then
    require("den.commands").subcommands.tasks(opts and opts.all and { "all" } or {})
  end
end

function M.inbox()
  if M.ensure() then
    require("den.commands").subcommands.inbox({})
  end
end

--- Opens the capture box, or saves `text` straight away.
function M.capture(text)
  if M.ensure() then
    require("den.commands").subcommands.capture(text and { text } or {})
  end
end

function M.today()
  if M.ensure() then
    require("den.commands").subcommands.today({})
  end
end

function M.find()
  if M.ensure() then
    require("den.commands").subcommands.find({})
  end
end

--- The statusline component (see lua/den/statusline.lua).
function M.statusline()
  return require("den.statusline").string()
end

function M.search()
  if M.ensure() then
    require("den.commands").subcommands.search({})
  end
end

return M
