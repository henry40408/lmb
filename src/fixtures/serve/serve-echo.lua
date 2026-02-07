function serve_echo(ctx)
  local request = ctx.request
  local json = require("@lmb/json")

  local raw = io.read("*a")
  local ok, parsed = pcall(function()
    return json.decode(raw)
  end)

  local body = raw
  if ok then
    body = parsed
  end

  return {
    body = {
      method = request.method,
      path = request.path,
      query = request.query,
      headers = request.headers,
      body = body,
    },
  }
end

return serve_echo
