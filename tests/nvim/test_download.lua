-- Installing a prebuilt engine: a fake release served from files, into a
-- scratch plugin folder.

local download = require("den.download")

local base = T.tmp .. "/download"
local release = base .. "/release"
local plugin = base .. "/plugin"
local target = download.target()

local function sh(cmd)
  local res = vim.system({ "sh", "-c", cmd }, { text = true }):wait()
  if res.code ~= 0 then
    error(cmd .. ": " .. (res.stderr or ""), 2)
  end
  return vim.trim(res.stdout or "")
end

local function write(path, text)
  vim.fn.mkdir(vim.fn.fnamemodify(path, ":h"), "p")
  local f = assert(io.open(path, "w"))
  f:write(text)
  f:close()
end

local function read(path)
  local f = io.open(path, "r")
  if not f then
    return nil
  end
  local text = f:read("*a")
  f:close()
  return text
end

local file = ("den-9.9.9-%s.tar.gz"):format(target)

--- Packs a release the way scripts/package.sh does.
local function publish(engine)
  vim.fn.delete(release, "rf")
  write(release .. "/stage/lua/den_native.so", engine)
  write(release .. "/stage/bin/den", "#!/bin/sh\necho den\n")
  sh(("cd '%s' && tar -czf '%s' -C stage lua bin"):format(release, file))
  sh(("cd '%s' && { sha256sum '%s' 2>/dev/null || shasum -a 256 '%s'; } > SHA256SUMS"):format(release, file, file))
end

local function install()
  local result
  download.install({ root = plugin, version = "9.9.9", url = "file://" .. release .. "/{file}" }, function(ok, msg)
    result = { ok = ok, msg = msg }
  end)
  vim.wait(10000, function()
    return result ~= nil
  end, 20)
  return result
end

T.test("the plugin and engine versions match", function()
  local cargo = read(T.root .. "/Cargo.toml")
  T.eq(require("den.version"), cargo:match('\nversion = "([^"]+)"'))
end)

T.test("a release for this machine is checked and installed", function()
  T.ok(target, "a known machine")
  publish("engine one")
  local r = install()
  T.ok(r.ok, r.msg)
  T.contains(r.msg, "engine 9.9.9")
  T.eq(read(plugin .. "/lua/den_native.so"), "engine one")
  T.ok(vim.fn.executable(plugin .. "/bin/den") == 1, "bin/den is executable")
end)

T.test("a file that does not match its checksum is not installed", function()
  publish("engine two")
  write(release .. "/SHA256SUMS", ("%s  %s\n"):format(string.rep("0", 64), file))
  local r = install()
  T.ok(not r.ok)
  T.contains(r.msg, "does not match its checksum")
  T.eq(read(plugin .. "/lua/den_native.so"), "engine one", "the old engine stays")
end)

T.test("a missing release says so", function()
  vim.fn.delete(release, "rf")
  local r = install()
  T.ok(not r.ok)
  T.contains(r.msg, "could not download SHA256SUMS")
end)

T.test("with allowed signers, only a signed release installs", function()
  local keys = base .. "/keys"
  vim.fn.mkdir(keys, "p")
  sh(("ssh-keygen -q -t ed25519 -N '' -C release -f '%s/good'"):format(keys))
  sh(("ssh-keygen -q -t ed25519 -N '' -C other -f '%s/bad'"):format(keys))
  local pub = read(keys .. "/good.pub"):match("^(%S+ %S+)")
  write(plugin .. "/release/allowed_signers", "den-release " .. pub .. "\n")

  publish("engine three")
  local unsigned = install()
  T.ok(not unsigned.ok)
  T.contains(unsigned.msg, "not signed")

  sh(("ssh-keygen -q -Y sign -n den-release -f '%s/bad' '%s/SHA256SUMS'"):format(keys, release))
  local wrong = install()
  T.ok(not wrong.ok)
  T.contains(wrong.msg, "signature does not check out")
  T.eq(read(plugin .. "/lua/den_native.so"), "engine one")

  os.remove(release .. "/SHA256SUMS.sig")
  sh(("ssh-keygen -q -Y sign -n den-release -f '%s/good' '%s/SHA256SUMS'"):format(keys, release))
  local good = install()
  T.ok(good.ok, good.msg)
  T.eq(read(plugin .. "/lua/den_native.so"), "engine three")
end)
