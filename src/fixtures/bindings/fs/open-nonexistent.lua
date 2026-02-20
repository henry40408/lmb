function open_nonexistent(ctx)
  local fs = require("@lmb/fs")
  local f, err = fs:open(ctx.state .. "/nonexistent.txt", "r")
  return err ~= nil
end
return open_nonexistent
