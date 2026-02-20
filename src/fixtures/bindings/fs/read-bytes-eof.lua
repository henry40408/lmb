function read_bytes_eof(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "r")
  -- Read past end of empty file
  local data = f:read(10)
  f:close()
  return data == nil
end
return read_bytes_eof
