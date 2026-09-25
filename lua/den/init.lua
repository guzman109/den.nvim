-- Den: projects, notes and next actions in a Markdown vault.
--
--   require("den").setup({
--     vault = "~/Notes/den",      -- optional; else ~/.config/den/config.yaml
--     ask_about_folders = true,   -- ask once about unknown code folders
--     images = true,              -- charts and focus rings as images in kitty
--   })
--
-- Break nudges have no option here, on purpose: see lua/den/nudges.lua.
--
-- Den adds no key mappings. Everything is under :Den, and the Lua API below
-- is there to map as you like.

local M = {}

M.options = {}
local started = false

function M.setup(opts)
  M.options = vim.tbl_extend("force", M.options, opts or {})
  require("den.highlights").setup()
  local ok, err = pcall(require("den.state").setup, {
    vault = M.options.vault,
    machine = M.options.machine,
    config = M.options.config,
  })
  if not ok then
    vim.notify("Den: " .. tostring(err):gsub("^runtime error: ", ""), vim.log.levels.ERROR)
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
