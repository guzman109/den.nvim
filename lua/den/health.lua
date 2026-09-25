-- :checkhealth den

local M = {}

function M.check()
  local h = vim.health
  h.start("Den engine")
  local native = require("den.native")
  local mod, why = native.get()
  if mod then
    h.ok("engine loaded (version " .. mod.version() .. ")")
  else
    h.error("engine not built", { "Run :Den build (needs cargo)", tostring(why) })
    return
  end

  local state = require("den.state")
  if state.info then
    h.ok("vault: " .. state.info.root)
    h.ok("machine name: " .. state.info.machine)
    if state.ready then
      local problems = native.call("problems") or {}
      if #problems == 0 then
        h.ok("every file reads cleanly")
      else
        for _, p in ipairs(problems) do
          h.warn(p[1] .. ": " .. p[2])
        end
      end
    else
      h.info("vault still loading")
    end
  else
    h.info("not started yet; run :Den or call require('den').setup()")
  end

  if state.ready then
    h.start("Den sync")
    local sync = require("den.sync")
    local settings = state.info.config.sync or {}
    local s = native.call("sync_status") or {}
    local snap = s.snapshot
    if settings.enabled == false then
      h.info("sync is off (sync.enabled: false)")
    elseif snap and not snap.repo then
      h.warn("the vault is not a git repository", { "Run: den init --remote <url>" })
    elseif snap then
      if snap.upstream then
        h.ok("syncs with " .. snap.upstream)
      else
        h.info("no upstream yet; the next sync pushes to the first remote, if there is one")
      end
      local waiting = (snap.changes or 0) + (snap.ahead or 0)
      h.info(("last sync: %s%s"):format(sync.describe(s.outcome), waiting > 0 and (" · " .. waiting .. " waiting") or ""))
      if #(snap.conflicts or {}) > 0 then
        h.warn("conflicts in " .. table.concat(snap.conflicts, ", "), { "Run :Den sync to settle them" })
      end
    end
    local program = sync.program()
    if program then
      h.ok("passphrase prompts inside Neovim use " .. program)
    else
      h.warn("the den command was not found", {
        ":Den sync cannot ask for an SSH passphrase inside Neovim",
        "Run :Den build (builds bin/den), or cargo install --path crates/den-cli",
      })
    end
  end

  h.start("Den companions")
  for _, dep in ipairs({
    { "markview", "draws [/], [-], #tags and frontmatter in notes" },
    { "fzf-lua", "pickers and search; falls back to vim.ui.select" },
  }) do
    if pcall(require, dep[1]) then
      h.ok(dep[1] .. ": " .. dep[2])
    else
      h.info(dep[1] .. " not found: " .. dep[2])
    end
  end
  for _, exe in ipairs({ { "git", "sync" }, { "rg", "search inside notes" } }) do
    if vim.fn.executable(exe[1]) == 1 then
      h.ok(exe[1] .. " found (" .. exe[2] .. ")")
    else
      h.warn(exe[1] .. " not found (" .. exe[2] .. ")")
    end
  end
end

return M
