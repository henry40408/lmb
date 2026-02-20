function write_no_perms(ctx)
  local fs = require("@lmb/fs")
  local f, err = fs:open(ctx.state, "w")
  return tostring(err)
end
return write_no_perms
