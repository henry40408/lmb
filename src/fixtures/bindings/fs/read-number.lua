function read_number(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "r")
  local n = f:read("*n")
  f:close()
  return n
end
return read_number
