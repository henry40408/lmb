function open_ap(ctx)
  local fs = require("@lmb/fs")
  local path = ctx.state
  local f = fs:open(path, "a+")
  f:write(" appended")
  f:seek("set", 0)
  local content = f:read("*a")
  f:close()
  return content
end
return open_ap
