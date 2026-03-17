function tail_basic(ctx)
  local fs = require("@lmb/fs")
  local result = {}
  for line in fs:tail(ctx.state, { from = "start" }) do
    table.insert(result, line)
    if #result >= 3 then
      break
    end
  end
  return result
end

return tail_basic
