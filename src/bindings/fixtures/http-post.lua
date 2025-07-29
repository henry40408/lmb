function http_post(ctx)
  local http = require("@lmb/http")

  local url = ctx.state
  local res = http:fetch(url, {
    method = "POST",
    headers = { ["x-api-key"] = "api-key" },
    body = "a",
  })
  local json = res:json()
  assert(res.ok, "Expected response to be ok, got " .. tostring(res.ok))
  assert(201 == res.status, "Expected status 201, got " .. res.status)
  assert("1" == res.headers["mocked"], "Expected header 'mocked' to be '1', got " .. res.headers["mocked"])
  return json
end

return http_post
