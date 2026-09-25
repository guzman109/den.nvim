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

T.test("the engine counts as old when its Rust code is newer", function()
  local native = require("den.native")
  local root = T.tmp .. "/plugin"
  vim.fn.mkdir(root .. "/lua", "p")
  vim.fn.mkdir(root .. "/crates/den-core/src", "p")
  vim.fn.mkdir(root .. "/crates/den-core/tests", "p")
  local function file(rel, at)
    vim.fn.writefile({ "x" }, root .. "/" .. rel)
    vim.uv.fs_utime(root .. "/" .. rel, at, at)
  end
  T.ok(native.stale(root), "nothing built yet")
  file("crates/den-core/src/lib.rs", 1000)
  file("Cargo.lock", 1000)
  file("lua/den_native.so", 2000)
  T.ok(not native.stale(root), "built after the code")
  file("crates/den-core/tests/vault.rs", 3000)
  T.ok(not native.stale(root), "tests are not part of the engine")
  file("crates/den-core/src/lib.rs", 3000)
  T.ok(native.stale(root), "a source file changed, as after an update")
  file("lua/den_native.so", 4000)
  file("Cargo.lock", 5000)
  T.ok(native.stale(root), "the lock file changed")
end)

T.test("setup waits for a build and never starts a second one", function()
  local native = require("den.native")
  local den = require("den")
  local real = { build = native.build, stale = native.stale, loaded = native.loaded, building = native.building }
  local builds, running = 0, false
  native.loaded = function()
    return false
  end
  native.stale = function()
    return true
  end
  native.building = function()
    return running
  end
  native.build = function()
    builds = builds + 1
    running = true
  end
  T.eq(den.setup({ build = true }), false, "not started while building")
  T.eq(den.setup(), false)
  T.eq(builds, 1, "one build")
  T.contains(T.notes[#T.notes], "still building")
  for k, v in pairs(real) do
    native[k] = v
  end
  den.options.build = false
end)

T.test("without cargo the engine is downloaded, and one install runs at a time", function()
  local native = require("den.native")
  local download = require("den.download")
  local real_cargo, real_install = native.cargo, download.install
  local installs, finish = 0, nil
  native.cargo = function()
    return nil
  end
  download.install = function(_, done)
    installs = installs + 1
    finish = done
  end
  local results = {}
  native.build(function(ok)
    table.insert(results, ok)
  end)
  native.build(function(ok)
    table.insert(results, ok)
  end)
  T.eq(installs, 1, "the second call joins the first")
  T.ok(native.building())
  finish(false, "no prebuilt engine for this machine")
  T.eq(results, { false, false })
  T.ok(not native.building())
  T.contains(T.notes[#T.notes], "Install Rust")
  native.cargo, download.install = real_cargo, real_install
end)
