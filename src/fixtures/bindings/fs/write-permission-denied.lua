function write_denied(ctx)
  local fs = require("@lmb/fs")
  local f, err = fs:open(ctx.state, "w")
  return err
end
return write_denied
