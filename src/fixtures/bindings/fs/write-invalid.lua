function write_invalid(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "w")
  local ok, err = pcall(function() f:write(true) end)
  f:close()
  return tostring(err)
end
return write_invalid
