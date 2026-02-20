function read_bytes(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "r")
  local bytes = f:read(5)
  f:close()
  return bytes
end

return read_bytes
