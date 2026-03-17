function tail_wait(ctx)
  local fs = require("@lmb/fs")
  local result = {}
  -- ctx.state is { path = "...", expected = N }
  for line in fs:tail(ctx.state.path, { from = "start", poll_interval = 50 }) do
    table.insert(result, line)
    if #result >= ctx.state.expected then
      break
    end
  end
  return result
end

return tail_wait
