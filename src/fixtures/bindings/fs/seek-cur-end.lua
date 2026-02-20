function seek_cur_end(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "r")
  -- seek to end
  local size = f:seek("end", 0)
  -- seek back from current
  f:seek("set", 0)
  f:seek("cur", 2)
  local rest = f:read("*a")
  f:close()
  return { size = size, rest = rest }
end
return seek_cur_end
