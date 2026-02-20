function open_append(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "a")
  f:write("second")
  f:close()

  local f2 = fs:open(ctx.state, "r")
  local content = f2:read("*a")
  f2:close()
  return content
end

return open_append
