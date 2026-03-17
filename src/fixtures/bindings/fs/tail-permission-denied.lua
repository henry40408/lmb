function tail_permission_denied(ctx)
  local fs = require("@lmb/fs")
  local ok, err = pcall(function()
    for line in fs:tail(ctx.state) do
      -- should never reach here
    end
  end)
  assert(not ok, "Expected error")
  local err_str = tostring(err)
  assert(string.find(err_str, "permission denied"), "Expected 'permission denied' in error: " .. err_str)
  return true
end

return tail_permission_denied
