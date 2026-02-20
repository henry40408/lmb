function read_denied_some(ctx)
  local fs = require("@lmb/fs")
  local f, err = fs:open(ctx.state, "r")
  return tostring(err)
end
return read_denied_some
