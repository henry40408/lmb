function flush_test(ctx)
  local fs = require("@lmb/fs")
  local path = ctx.state
  local f = fs:open(path, "w")
  f:write("flushed")
  f:flush()
  f:close()
  local f2 = fs:open(path, "r")
  local content = f2:read("*a")
  f2:close()
  return content
end
return flush_test
