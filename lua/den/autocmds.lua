-- Keeping Den in step with the editor.

local native = require("den.native")
local state = require("den.state")
local apply = require("den.apply")
local decor = require("den.decor")
local screen = require("den.ui.screen")

local M = {}

function M.setup(opts)
  local group = vim.api.nvim_create_augroup("den", { clear = true })

  -- A saved vault file: Den forgets the buffer's overlay and re-reads it.
  vim.api.nvim_create_autocmd("BufWritePost", {
    group = group,
    callback = function(ev)
      if not state.ready then
        return
      end
      local rel = native.call("rel", vim.api.nvim_buf_get_name(ev.buf))
      if rel then
        apply.clear_overlay(rel)
        native.call("reload", rel)
        state.changed({ rel })
      end
    end,
  })

  -- Which vault files have unsaved changes, for other writers (an agent's
  -- MCP server, the `den` command) to keep off. Updated whenever a
  -- buffer's modified flag changes, and cleared on the way out.
  local function publish()
    local paths = {}
    for _, buf in ipairs(vim.api.nvim_list_bufs()) do
      if vim.api.nvim_buf_is_loaded(buf) and vim.bo[buf].modified and vim.bo[buf].buftype == "" then
        local rel = native.call("rel", vim.api.nvim_buf_get_name(buf))
        if rel then
          table.insert(paths, rel)
        end
      end
    end
    native.call("publish_editing", paths)
  end
  M.publish_editing = publish
  vim.api.nvim_create_autocmd({ "BufModifiedSet", "BufWritePost", "BufDelete" }, {
    group = group,
    callback = function()
      if state.ready then
        vim.schedule(publish)
      end
    end,
  })
  vim.api.nvim_create_autocmd("VimLeavePre", {
    group = group,
    callback = function()
      if state.ready then
        native.call("publish_editing", {})
      end
    end,
  })

  -- Decorations follow edits and new windows.
  vim.api.nvim_create_autocmd({ "BufWinEnter", "TextChanged", "InsertLeave" }, {
    group = group,
    pattern = "*.md",
    callback = function(ev)
      if state.ready then
        decor.schedule(ev.buf)
      end
    end,
  })

  -- Screens redraw on resize.
  vim.api.nvim_create_autocmd("VimResized", {
    group = group,
    callback = function()
      screen.render_all()
      require("den.focus").resized()
    end,
  })

  -- The one-time question about an unknown code folder.
  if opts.ask_about_folders ~= false then
    vim.api.nvim_create_autocmd("DirChanged", {
      group = group,
      callback = function()
        if state.ready then
          require("den.folder").check()
        end
      end,
    })
    state.when_ready(function()
      if vim.v.vim_did_enter == 1 then
        require("den.folder").check()
      else
        vim.api.nvim_create_autocmd("VimEnter", {
          group = group,
          once = true,
          callback = function()
            require("den.folder").check()
          end,
        })
      end
    end)
  end

  -- News from the engine (a load, a pull, another editor, Den itself).
  state.on_change(function()
    screen.render_all()
    decor.refresh()
    vim.cmd("redrawstatus")
  end)
end

return M
