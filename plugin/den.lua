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

-- After vim.pack updates Den, rebuild the engine in the background (cargo
-- compiles only what changed), so the next start loads a current one.
pcall(vim.api.nvim_create_autocmd, "PackChanged", {
  group = vim.api.nvim_create_augroup("den.build", { clear = true }),
  callback = function(ev)
    local data = ev.data or {}
    if data.kind ~= "update" or not data.path then
      return
    end
    local native = require("den.native")
    if vim.uv.fs_realpath(data.path) == vim.uv.fs_realpath(native.root) then
      native.build()
    end
  end,
})
