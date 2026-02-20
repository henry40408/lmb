function write_number(ctx)
  local fs = require("@lmb/fs")
  local path = ctx.state
  local f = fs:open(path, "w")
  f:write(42)
  f:write(3.14)
  f:close()
  local f2 = fs:open(path, "r")
  local content = f2:read("*a")
  f2:close()
  return content
end
return write_number
