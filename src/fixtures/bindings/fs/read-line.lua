function read_line(ctx)
  local fs = require("@lmb/fs")
  local f = fs:open(ctx.state, "r")
  local lines = {}
  while true do
    local line = f:read("*l")
    if line == nil then break end
    table.insert(lines, line)
  end
  f:close()
  return lines
end

return read_line
