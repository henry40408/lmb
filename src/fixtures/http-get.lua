function http_get(ctx)
  local m = require("@lmb/http")
  local res = m:fetch(ctx.state, { headers = { ["user-agent"] = "curl/1.0" } })
  assert(res.status == 200, "Expected status 200, got " .. res.status)
end

return http_get
