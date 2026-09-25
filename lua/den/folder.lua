-- The first time Neovim opens in a code folder that no project knows about,
-- Den asks once: create a project for it, link it to one, or ignore it.

local native = require("den.native")
local apply = require("den.apply")

local M = {}

local function store_path()
  return vim.fn.stdpath("state") .. "/den/folders.json"
end

local function load()
  local f = io.open(store_path(), "r")
  if not f then
    return {}
  end
  local ok, data = pcall(vim.json.decode, f:read("*a"))
  f:close()
  return ok and type(data) == "table" and data or {}
end

local function save(data)
  vim.fn.mkdir(vim.fn.fnamemodify(store_path(), ":h"), "p")
  local f = io.open(store_path(), "w")
  if f then
    f:write(vim.json.encode(data))
    f:close()
  end
end

--- Remembers that `top` was asked about.
function M.remember(top, answer)
  local data = load()
  data[top] = answer
  save(data)
end

--- The top of the git checkout Neovim is in, if it needs asking about.
function M.candidate(dir)
  local top = vim.fs.root(dir, ".git")
  if not top then
    return nil
  end
  if native.call("project_for_dir", dir) then
    return nil
  end
  local vault = native.call("root")
  if vault and vim.startswith(vim.fn.resolve(top), vim.fn.resolve(vault)) then
    return nil
  end
  if load()[top] then
    return nil
  end
  return top
end

--- Shows the question for `top` in a small window.
function M.ask(top)
  local name = vim.fn.fnamemodify(top, ":t")
  local lines = {
    "  This folder isn't a Den project yet",
    "  " .. vim.fn.fnamemodify(top, ":~"),
    "",
    "  c  create project “" .. name .. "”",
    "  l  link to an existing project…",
    "  i  ignore this folder",
  }
  local buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false
  vim.bo[buf].bufhidden = "wipe"
  local width = 0
  for _, l in ipairs(lines) do
    width = math.max(width, vim.fn.strdisplaywidth(l) + 4)
  end
  local win = vim.api.nvim_open_win(buf, true, {
    relative = "editor",
    width = width,
    height = #lines,
    row = math.floor((vim.o.lines - #lines) / 3),
    col = math.floor((vim.o.columns - width) / 2),
    style = "minimal",
    border = "rounded",
    footer = " asked once · later: :Den project ",
    footer_pos = "right",
  })
  local ns = vim.api.nvim_create_namespace("den.folder")
  vim.api.nvim_buf_set_extmark(buf, ns, 0, 0, { end_col = #lines[1], hl_group = "DenHeader" })
  vim.api.nvim_buf_set_extmark(buf, ns, 1, 0, { end_col = #lines[2], hl_group = "DenMuted" })
  for i = 3, 5 do
    vim.api.nvim_buf_set_extmark(buf, ns, i, 2, { end_col = 3, hl_group = "DenKey" })
  end
  M.remember(top, "asked")
  local function close()
    if vim.api.nvim_win_is_valid(win) then
      vim.api.nvim_win_close(win, true)
    end
  end
  local map = function(lhs, fn)
    vim.keymap.set("n", lhs, function()
      close()
      fn()
    end, { buffer = buf, nowait = true })
  end
  map("c", function()
    if apply.run(function(mod)
      return mod.plan_new_project(name, top)
    end) then
      vim.notify("Den: created project " .. name)
    end
  end)
  map("l", function()
    require("den.pick").project({ prompt = "Link " .. name .. " to" }, function(p)
      if apply.run(function(mod)
        return mod.plan_link(p.name, top)
      end) then
        vim.notify("Den: linked to " .. p.title)
      end
    end)
  end)
  map("i", function()
    M.remember(top, "ignored")
  end)
  map("q", function() end)
  map("<Esc>", function() end)
  return win
end

--- Asks about the current folder if it needs asking.
function M.check()
  local top = M.candidate(vim.fn.getcwd())
  if top then
    M.ask(top)
  end
end

return M
