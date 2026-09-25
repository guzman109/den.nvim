-- Den's highlight groups. Each links to a standard group, so any colour
-- scheme gives sensible colours; set any `Den*` group yourself to override.

local M = {}

M.groups = {
  DenHeader = { link = "Title" },
  DenGroup = { bold = true },
  DenGroupDoing = { link = "Special" },
  DenMuted = { link = "Comment" },
  DenKey = { link = "Special" },
  DenOpen = { link = "Comment" },
  DenDoing = { link = "DiagnosticWarn" },
  DenDone = { link = "DiagnosticOk" },
  DenDropped = { link = "Comment" },
  DenDue = { link = "DiagnosticWarn" },
  DenDueToday = { link = "DiagnosticWarn" },
  DenOverdue = { link = "DiagnosticError" },
  DenTag = { link = "Identifier" },
  DenProject = { link = "Comment" },
  DenNote = { link = "Underlined" },
  DenTimer = { link = "DiagnosticWarn" },
  DenNudge = { link = "DiagnosticError" },
  DenSync = { link = "Comment" },
  DenSyncProblem = { link = "DiagnosticError" },
}

function M.setup()
  for name, spec in pairs(M.groups) do
    vim.api.nvim_set_hl(0, name, vim.tbl_extend("force", { default = true }, spec))
  end
end

return M
