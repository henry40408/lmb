function seek_test(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "r")
  f:seek("set", 5)
  local content = f:read("*a")
  f:close()
  return content
end

return seek_test
