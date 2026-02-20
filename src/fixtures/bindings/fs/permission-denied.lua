function permission_denied()
  local fs = require("@lmb/fs")

  -- open should return nil + error when no permissions
  local f, err = fs:open("/tmp/test.txt", "r")
  assert(f == nil, "Expected nil handle when permission denied")
  assert(err ~= nil, "Expected error message when permission denied")
  assert(string.find(err, "permission denied"), "Expected 'permission denied' in error: " .. err)
end

return permission_denied
