function tail_rotation(ctx)
  local fs = require("@lmb/fs")
  local result = {}
  for line in fs:tail(ctx.state.path, { from = "start", poll_interval = 50 }) do
    table.insert(result, line)
    if #result >= ctx.state.expected then
      break
    end
  end
  return result
end

return tail_rotation
