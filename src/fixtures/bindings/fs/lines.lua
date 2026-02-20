function lines_test(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "r")
  local result = {}
  for line in f:lines() do
    table.insert(result, line)
  end
  f:close()
  return result
end

return lines_test
