function open_invalid(ctx)
  local fs = require("@lmb/fs")
  local f, err = fs:open(ctx.state, "x")
  return err
end
return open_invalid
