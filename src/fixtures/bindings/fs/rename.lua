function rename_test(ctx)
  local fs = require("@lmb/fs")
  fs:rename(ctx.state.old, ctx.state.new)
end

return rename_test
