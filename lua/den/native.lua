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

--- Calls into the engine, turning an engine error into (nil, message).
function M.call(name, ...)
  local mod = M.need()
  local ok, result = pcall(mod[name], ...)
  if ok then
    return result
  end
  return nil, (tostring(result):gsub("^runtime error: ", ""))
end

--- Installs the engine: the prebuilt release for this version when there
--- is one, else a build with cargo (`from_source` skips the download).
--- Restart Neovim afterwards to load it.
function M.build(on_done, from_source)
  if not from_source then
    vim.notify("Den: downloading the engine…")
    require("den.download").install({}, function(ok, message)
      if ok then
        vim.notify("Den: " .. message .. ". Restart Neovim to load it.")
        if on_done then
          on_done(true)
        end
      elseif vim.fn.executable("cargo") == 1 then
        vim.notify("Den: " .. message .. "; building from source instead")
        M.build(on_done, true)
      else
        vim.notify("Den: " .. message .. ", and cargo is not installed to build it", vim.log.levels.ERROR)
        if on_done then
          on_done(false)
        end
      end
    end)
    return
  end
  local script = M.root .. "/scripts/build-nvim.sh"
  vim.notify("Den: building the engine…")
  vim.system({ "sh", script }, { cwd = M.root, text = true }, function(res)
    vim.schedule(function()
      if res.code == 0 then
        vim.notify("Den: engine built. Restart Neovim to load it.")
      else
        vim.notify("Den: build failed\n" .. (res.stderr or ""), vim.log.levels.ERROR)
      end
      if on_done then
        on_done(res.code == 0)
      end
    end)
  end)
end

return M
