function exists_test(ctx)
  local fs = require("@lmb/fs")
  assert(fs:exists(ctx.state) == true, "Expected file to exist")
  assert(fs:exists("/nonexistent/path/abc123") == false, "Expected nonexistent path to return false")
end

return exists_test
