function open_read(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "r")
  local content = f:read("*a")
  f:close()
  return content
end

return open_read
