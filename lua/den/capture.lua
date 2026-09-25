-- Capture: a one-line box over whatever you are editing. The thought lands
-- in the Inbox of the project for the folder Neovim is in, or in the general
-- inbox outside any project.

local native = require("den.native")
local apply = require("den.apply")
local state = require("den.state")

local M = {}

local function project_here()
  return native.call("project_for_dir", vim.fn.getcwd())
end

--- Saves `text` straight away. `project` is a project name, or false for the
--- general inbox; nil means "the project for this folder".
function M.save(text, project)
  if project == nil then
    local p = project_here()
    project = p and p.name or false
  end
  local ok = apply.run(function(mod)
    return mod.plan_capture(project or nil, text)
  end)
  if ok then
    vim.notify("Den: captured to " .. (project or "inbox"))
  end
  return ok
end

--- Opens the capture box.
function M.open(opts)
  opts = opts or {}
  if not state.ready then
    vim.notify("Den is still loading your vault")
    return
  end
  local target = opts.project
  local title_of = { [false] = "inbox" }
  if target == nil then
    local p = project_here()
    target = p and p.name or false
    if p then
      title_of[p.name] = p.title
    end
  end

  local buf = vim.api.nvim_create_buf(false, true)
  vim.bo[buf].bufhidden = "wipe"
  local width = math.max(60, math.min(math.floor(vim.o.columns * 0.6), 110, vim.o.columns - 4))
  local function title()
    return " capture → " .. (title_of[target] or target) .. " "
  end
  local win = vim.api.nvim_open_win(buf, true, {
    relative = "editor",
    width = width,
    height = 1,
    row = vim.o.lines - 6,
    col = math.floor((vim.o.columns - width) / 2),
    style = "minimal",
    border = "rounded",
    title = title(),
    title_pos = "left",
    footer = " enter save · tab another project · esc cancel ",
    footer_pos = "right",
  })
  vim.cmd("startinsert")

  local function close()
    vim.cmd("stopinsert")
    if vim.api.nvim_win_is_valid(win) then
      vim.api.nvim_win_close(win, true)
    end
  end
  local function save()
    local text = vim.api.nvim_buf_get_lines(buf, 0, 1, false)[1] or ""
    close()
    if vim.trim(text) ~= "" then
      M.save(text, target)
    end
  end
  local function pick()
    vim.cmd("stopinsert")
    require("den.pick").project({ prompt = "Capture to" }, function(p)
      target = p.name
      title_of[p.name] = p.title
      if vim.api.nvim_win_is_valid(win) then
        vim.api.nvim_win_set_config(win, { title = title(), title_pos = "left" })
        vim.api.nvim_set_current_win(win)
        vim.cmd("startinsert!")
      end
    end)
  end
  local map = function(modes, lhs, fn)
    vim.keymap.set(modes, lhs, fn, { buffer = buf, nowait = true })
  end
  map({ "i", "n" }, "<CR>", save)
  map({ "i", "n" }, "<Tab>", pick)
  map("n", "<Esc>", close)
  map("n", "q", close)
  map("i", "<C-c>", close)
  vim.api.nvim_create_autocmd("WinLeave", {
    buffer = buf,
    once = true,
    callback = function()
      vim.schedule(function()
        local cur = vim.api.nvim_get_current_win()
        if cur ~= win and vim.api.nvim_win_is_valid(win) and vim.bo[vim.api.nvim_win_get_buf(cur)].filetype ~= "fzf" then
          close()
        end
      end)
    end,
  })
  return buf, win
end

return M
