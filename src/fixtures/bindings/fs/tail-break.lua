function tail_break(ctx)
  local fs = require("@lmb/fs")
  local result = {}
  for line in fs:tail(ctx.state, { from = "start" }) do
    table.insert(result, line)
    if #result >= 2 then
      break
    end
  end
  return result
end

return tail_break
