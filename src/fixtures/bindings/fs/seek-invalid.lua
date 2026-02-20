function seek_invalid(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "r")
  local ok, err = pcall(function() f:seek("bad", 0) end)
  f:close()
  return tostring(err)
end
return seek_invalid
