function http_post(ctx)
  local http = require("@lmb/http")

  local url = ctx.state
  local res = http:fetch(url, {
    method = "POST",
    headers = { ["x-api-key"] = "api-key" },
    body = "a",
  })
  return res:json()
end

return http_post
