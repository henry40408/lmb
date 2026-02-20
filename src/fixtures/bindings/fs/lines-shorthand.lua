function lines_shorthand(ctx)
  local fs = require("@lmb/fs")
  local result = {}
  for line in fs:lines(ctx.state) do
    table.insert(result, line)
  end
  return result
end

return lines_shorthand
