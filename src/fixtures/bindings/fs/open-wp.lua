function open_wp(ctx)
  local fs = require("@lmb/fs")
  local path = ctx.state
  local f = fs:open(path, "w+")
  f:write("hello w+")
  f:seek("set", 0)
  local content = f:read("*a")
  f:close()
  return content
end
return open_wp
