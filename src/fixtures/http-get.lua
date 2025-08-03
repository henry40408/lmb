function http_get()
  local m = require("@lmb/http")
  local res = m:fetch("https://httpbingo.org", { headers = { ["user-agent"] = "curl/1.0" } })
  assert(res.status == 200, "Expected status 200, got " .. res.status)
end

return http_get
