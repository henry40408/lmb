function read_number_nan(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "r")
  local n = f:read("*n")
  f:close()
  -- n should be nil for non-numeric content
  return n == nil
end
return read_number_nan
