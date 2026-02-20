function type_test(ctx)
  local fs = require("@lmb/fs")

  -- non-file value
  assert(fs.type("hello") == nil, "Expected nil for string")
  assert(fs.type(123) == nil, "Expected nil for number")

  -- open file handle
  local f = fs:open(ctx.state, "r")
  assert(fs.type(f) == "file", "Expected 'file' for open handle")

  -- closed file handle
  f:close()
  assert(fs.type(f) == "closed file", "Expected 'closed file' for closed handle")
end

return type_test
