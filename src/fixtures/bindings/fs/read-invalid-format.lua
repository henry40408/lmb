function read_invalid(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "r")
  local ok, err = pcall(function() f:read("*z") end)
  f:close()
  return tostring(err)
end
return read_invalid
