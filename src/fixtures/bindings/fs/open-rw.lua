function open_rw(ctx)
  local fs = require("@lmb/fs")
  local path = ctx.state
  local f = fs:open(path, "r+")
  local content = f:read("*a")
  f:seek("set", 0)
  f:write("XX")
  f:close()
  local f2 = fs:open(path, "r")
  local result = f2:read("*a")
  f2:close()
  return { original = content, modified = result }
end
return open_rw
