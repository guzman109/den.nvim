-- Loads the engine: the `den_native` library built from crates/den-nvim.

local M = {}

--- This plugin's root folder.
M.root = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h:h:h")

local lib

--- The native module, or nil and why it could not be loaded.
function M.get()
  if lib then
    return lib
  end
  local cpath = M.root .. "/lua/?.so"
  if not package.cpath:find(cpath, 1, true) then
    package.cpath = cpath .. ";" .. package.cpath
  end
  local ok, mod = pcall(require, "den_native")
  if not ok then
    return nil, mod
  end
  lib = mod
  return lib
end

--- The native module, or an error telling the person how to build it.
function M.need()
  local mod, why = M.get()
  if not mod then
    error("Den's engine is not built yet. Run :Den build\n" .. tostring(why), 0)
  end
  return mod
end

--- An engine error as the person should read it: without the Lua error
--- kind in front or the stack traceback behind.
function M.message(err)
  local text = tostring(err):gsub("^runtime error: ", "")
  text = text:gsub("\nstack traceback:.*$", "")
  return text
end

--- Calls into the engine, turning an engine error into (nil, message).
function M.call(name, ...)
  local mod = M.need()
  local ok, result = pcall(mod[name], ...)
  if ok then
    return result
  end
  return nil, M.message(result)
end

--- Whether the library is loaded in this session.
function M.loaded()
  return lib ~= nil
end

--- Whether the engine needs building: it is missing, or a Rust source,
--- manifest or the lock file is newer than it, as after an update (git
--- writes the files it changes with the current time). `root` defaults to
--- this plugin.
function M.stale(root)
  root = root or M.root
  local built = vim.uv.fs_stat(root .. "/lua/den_native.so")
  if not built then
    return true
  end
  local at = built.mtime.sec
  local function newer(path)
    local stat = vim.uv.fs_stat(path)
    return stat ~= nil and stat.mtime.sec > at
  end
  for _, file in ipairs({ "Cargo.lock", "Cargo.toml", "rust-toolchain.toml" }) do
    if newer(root .. "/" .. file) then
      return true
    end
  end
  local crates = root .. "/crates"
  local skip = function(dir)
    local name = vim.fs.basename(dir)
    return name ~= "target" and name ~= "tests" and name:sub(1, 1) ~= "."
  end
  for name, kind in vim.fs.dir(crates, { depth = 8, skip = skip }) do
    if kind == "file" and (name:match("%.rs$") or name:match("Cargo%.toml$")) and newer(crates .. "/" .. name) then
      return true
    end
  end
  return false
end

--- The cargo to build with: on PATH, else rustup's usual place (a Neovim
--- started from the desktop may not have ~/.cargo/bin on its PATH).
function M.cargo()
  if vim.fn.executable("cargo") == 1 then
    return vim.fn.exepath("cargo")
  end
  local home = vim.fs.normalize("~/.cargo/bin/cargo")
  if vim.fn.executable(home) == 1 then
    return home
  end
  return nil
end

local building = nil

--- Whether a build is running.
function M.building()
  return building ~= nil
end

local function finish(ok, message)
  local waiting = building or {}
  building = nil
  if not ok then
    vim.notify("Den: " .. message, vim.log.levels.ERROR)
  elseif lib then
    vim.notify("Den: " .. message .. ". Restart Neovim to use it.")
  else
    vim.notify("Den: " .. message .. ".")
  end
  for _, fn in pairs(waiting) do
    pcall(fn, ok)
  end
end

--- Installs the engine (the Neovim module, `den`, `den-agent` and
--- `den-mcp`), in the background. With cargo, it is built from this
--- checkout's source (scripts/build-nvim.sh), so it always matches the
--- code; cargo keeps its cache in the plugin's target/ folder, so after an
--- update only what changed is compiled. Without cargo, or with `how` =
--- "download", the prebuilt release for this version is downloaded
--- instead. `on_done(ok)` runs after; a build already running is joined,
--- not started twice.
function M.build(on_done, how)
  if building then
    table.insert(building, on_done)
    return
  end
  building = { on_done }
  local cargo = how ~= "download" and M.cargo()
  if not cargo then
    vim.notify("Den: downloading the engine…")
    require("den.download").install({}, function(ok, message)
      if ok then
        finish(true, message)
      elseif how == "download" then
        finish(false, message)
      else
        finish(false, message .. ". Install Rust (https://rustup.rs) to build it instead, then run :Den build")
      end
    end)
    return
  end
  local first = vim.uv.fs_stat(M.root .. "/target") == nil
  vim.notify("Den: building the engine with cargo" .. (first and " (the first build takes a few minutes)…" or "…"))
  local env = { PATH = vim.fs.dirname(cargo) .. ":" .. (vim.env.PATH or "") }
  vim.system({ "sh", M.root .. "/scripts/build-nvim.sh" }, { cwd = M.root, text = true, env = env }, function(res)
    vim.schedule(function()
      if res.code == 0 then
        finish(true, "engine built")
      else
        local lines = vim.split(vim.trim(res.stderr or ""), "\n")
        finish(false, "the engine did not build\n" .. table.concat(vim.list_slice(lines, math.max(1, #lines - 15)), "\n"))
      end
    end)
  end)
end

return M
