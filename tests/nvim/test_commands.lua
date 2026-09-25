local function with_select(choose, fn)
  local original = vim.ui.select
  local seen
  vim.ui.select = function(items, opts, on_choice)
    seen = { items = items, opts = opts }
    on_choice(choose(items))
  end
  package.loaded["fzf-lua"] = nil
  local ok, err = pcall(fn)
  vim.ui.select = original
  if not ok then
    error(err, 0)
  end
  return seen
end

T.test(":Den with no arguments opens Tasks", function()
  vim.cmd("Den")
  T.contains(vim.api.nvim_buf_get_name(0), "den://tasks")
end)

T.test(":Den tasks all and :Den inbox open their screens", function()
  vim.cmd("Den tasks all")
  T.contains(T.lines()[1], "all projects")
  vim.cmd("Den inbox")
  T.contains(vim.api.nvim_buf_get_name(0), "den://inbox")
end)

T.test(":Den capture with text saves without a box", function()
  vim.cmd("Den capture Straight from the command line")
  T.contains(T.read("inbox.md"), "- [ ] Straight from the command line\n")
end)

T.test(":Den completes its subcommands", function()
  local names = require("den.commands").complete("in", "Den in")
  T.eq(names, { "inbox" })
end)

T.test("find opens the chosen file (vim.ui.select when fzf-lua is absent)", function()
  local seen = with_select(function(items)
    for _, item in ipairs(items) do
      if item.path == "notes/kitty-graphics.md" then
        return item
      end
    end
  end, function()
    require("den.pick").find()
  end)
  T.ok(#seen.items > 5, "every readable file is offered")
  T.contains(vim.api.nvim_buf_get_name(0), "notes/kitty-graphics.md")
end)

T.test("m moves a task to the project chosen in the picker", function()
  local native = require("den.native")
  local view = native.call("inbox_view")
  local row
  for _, g in ipairs(view.groups) do
    for _, r in ipairs(g.tasks) do
      if r.title == "Find the old logo files" then
        row = r
      end
    end
  end
  with_select(function(items)
    for _, item in ipairs(items) do
      if item.name == "haste" then
        return item
      end
    end
  end, function()
    require("den.actions").move_pick(row)
  end)
  T.contains(T.read("projects/haste.md"), "- [ ] Find the old logo files\n")
  T.lacks(T.read("projects/website.md"), "Find the old logo files")
end)

T.test(":checkhealth den reports on the engine and sync", function()
  -- Den's checks run directly, collecting what they report: wiping a real
  -- :checkhealth buffer sometimes crashes Neovim 0.12.5 (BUGS.md, B-004).
  local seen = {}
  local fake = {}
  for _, level in ipairs({ "start", "ok", "info", "warn", "error" }) do
    fake[level] = function(msg)
      table.insert(seen, level .. " " .. msg)
    end
  end
  local real = vim.health
  vim.health = fake
  local ok, err = pcall(require("den.health").check)
  vim.health = real
  T.ok(ok, tostring(err))
  T.contains(seen, "ok engine loaded")
  T.contains(seen, "start Den sync")
  T.contains(seen, "start Den nudges and images")
end)
