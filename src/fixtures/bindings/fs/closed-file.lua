function closed_file(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "r")
  f:close()

  -- read on closed file should error
  local ok, err = pcall(function() f:read("*a") end)
  assert(not ok, "Expected error on read after close")
  assert(string.find(tostring(err), "closed file"), "Expected 'closed file' in error: " .. tostring(err))

  -- write on closed file should error
  ok, err = pcall(function() f:write("test") end)
  assert(not ok, "Expected error on write after close")
  assert(string.find(tostring(err), "closed file"), "Expected 'closed file' in error: " .. tostring(err))

  -- close again should error
  ok, err = pcall(function() f:close() end)
  assert(not ok, "Expected error on double close")
  assert(string.find(tostring(err), "closed file"), "Expected 'closed file' in error: " .. tostring(err))
end

return closed_file
