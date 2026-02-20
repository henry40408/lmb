function remove_test(ctx)
  local fs = require("@lmb/fs")
  fs:remove(ctx.state)
end

return remove_test
