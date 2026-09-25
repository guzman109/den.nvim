if vim.g.loaded_den then
  return
end
vim.g.loaded_den = true

vim.api.nvim_create_user_command("Den", function(cmd)
  require("den.commands").run(cmd)
end, {
  nargs = "*",
  desc = "Den: tasks, inbox, capture, notes, journal",
  complete = function(arglead, cmdline)
    return require("den.commands").complete(arglead, cmdline)
  end,
})
