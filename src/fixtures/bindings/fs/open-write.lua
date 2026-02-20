function open_write(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "w")
  f:write("hello from lua")
  f:close()

  local f2 = fs:open(ctx.state, "r")
  local content = f2:read("*a")
  f2:close()
  return content
end

return open_write
