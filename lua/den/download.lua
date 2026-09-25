-- Installing a prebuilt engine instead of building it.
--
-- Downloads `den-<version>-<target>.tar.gz` for this plugin's version and
-- this machine, checks it against the release's SHA256SUMS (and, when the
-- plugin carries release/allowed_signers and the release is signed, checks
-- the signature with ssh-keygen), then puts lua/den_native.so and bin/den in
-- place. Nothing is installed unless every check passes.

local M = {}

--- Where releases live. `{version}` and `{file}` are filled in. Moving the
--- project to another host means changing this one line.
M.url = "https://gitlab.com/api/v4/projects/cguz109%2FDen/packages/generic/den/{version}/{file}"

--- This machine's release target, or nil.
function M.target()
  local u = vim.uv.os_uname()
  local arch = (u.machine == "arm64" or u.machine == "aarch64") and "aarch64" or (u.machine == "x86_64" and "x86_64")
  if not arch then
    return nil
  end
  if u.sysname == "Darwin" then
    return arch .. "-apple-darwin"
  elseif u.sysname == "Linux" then
    return arch .. "-unknown-linux-gnu"
  end
  return nil
end

local function url(template, version, file)
  return (template:gsub("{version}", version):gsub("{file}", file))
end

local function run(cmd, opts, cb)
  vim.system(cmd, vim.tbl_extend("force", { text = true }, opts or {}), function(res)
    vim.schedule(function()
      cb(res.code == 0, res)
    end)
  end)
end

local function sha256(path, cb)
  local cmd = vim.fn.executable("sha256sum") == 1 and { "sha256sum", path } or { "shasum", "-a", "256", path }
  run(cmd, nil, function(ok, res)
    cb(ok and res.stdout:match("^(%x+)") or nil)
  end)
end

--- Downloads and installs. `opts`: `root` (the plugin folder), `version`,
--- `url` (a template like `M.url`). Calls `done(ok, message)`.
function M.install(opts, done)
  opts = opts or {}
  local root = opts.root or require("den.native").root
  local version = opts.version or require("den.version")
  local template = opts.url or M.url
  local target = M.target()
  if not target then
    return done(false, "no prebuilt engine for this machine")
  end
  local file = ("den-%s-%s.tar.gz"):format(version, target)
  local dir = vim.fn.tempname()
  vim.fn.mkdir(dir, "p")
  local function fail(msg)
    vim.fn.delete(dir, "rf")
    done(false, msg)
  end
  local function fetch(name, cb, optional)
    run({ "curl", "-fsSL", "--proto", "=https,file", "--proto-redir", "=https", "-o", dir .. "/" .. name, url(template, version, name) }, nil, function(ok, res)
      if not ok and not optional then
        return fail(("could not download %s (%s)"):format(name, vim.trim(res.stderr or "")))
      end
      cb(ok)
    end)
  end
  fetch("SHA256SUMS", function()
    fetch(file, function()
      local sums = io.open(dir .. "/SHA256SUMS"):read("*a")
      local want = sums:match("(%x+)%s+%*?" .. vim.pesc(file) .. "\n") or sums:match("(%x+)%s+%*?" .. vim.pesc(file) .. "$")
      if not want then
        return fail(file .. " is not in the release's SHA256SUMS")
      end
      sha256(dir .. "/" .. file, function(got)
        if got ~= want then
          return fail(file .. " does not match its checksum; nothing was installed")
        end
        local signers = root .. "/release/allowed_signers"
        local function unpack()
          run({ "tar", "-xzf", dir .. "/" .. file, "-C", dir }, nil, function(ok, res)
            if not ok then
              return fail("could not unpack " .. file .. ": " .. vim.trim(res.stderr or ""))
            end
            vim.fn.mkdir(root .. "/lua", "p")
            vim.fn.mkdir(root .. "/bin", "p")
            -- Every file is copied in beside its old one first; only when
            -- all are there does each replace the old one, so a failure
            -- never leaves a mix of versions.
            local files = vim.tbl_filter(function(file)
              return vim.uv.fs_stat(dir .. "/" .. file) ~= nil
            end, { "lua/den_native.so", "bin/den", "bin/den-agent" })
            local moved = true
            for _, file in ipairs(files) do
              local staged = root .. "/" .. file .. ".new"
              os.remove(staged)
              moved = moved and vim.uv.fs_copyfile(dir .. "/" .. file, staged) and true
              vim.uv.fs_chmod(staged, 493)
            end
            if moved then
              for _, file in ipairs(files) do
                -- A new file rather than an overwrite: macOS caches code
                -- signatures per file.
                os.remove(root .. "/" .. file)
                moved = moved and vim.uv.fs_rename(root .. "/" .. file .. ".new", root .. "/" .. file) and true
              end
            else
              for _, file in ipairs(files) do
                os.remove(root .. "/" .. file .. ".new")
              end
            end
            vim.fn.delete(dir, "rf")
            if not moved then
              return done(false, "could not copy the engine into " .. root)
            end
            done(true, ("engine %s (%s) installed"):format(version, target))
          end)
        end
        if vim.fn.filereadable(signers) == 0 then
          return unpack()
        end
        fetch("SHA256SUMS.sig", function(have_sig)
          if not have_sig then
            return fail("the release is not signed, and this plugin expects signed releases")
          end
          local sums_file = io.open(dir .. "/SHA256SUMS", "rb")
          run({ "ssh-keygen", "-Y", "verify", "-f", signers, "-I", "den-release", "-n", "den-release", "-s", dir .. "/SHA256SUMS.sig" }, { stdin = sums_file:read("*a") }, function(ok)
            sums_file:close()
            if not ok then
              return fail("the release's signature does not check out; nothing was installed")
            end
            unpack()
          end)
        end, true)
      end)
    end)
  end)
end

return M
