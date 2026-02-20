function read_bytes_partial(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "r")
  -- Request more bytes than available, should truncate
  local data = f:read(100)
  f:close()
  return data
end
return read_bytes_partial
