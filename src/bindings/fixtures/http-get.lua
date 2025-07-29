function http_get(ctx)
  local url = ctx.state
  local http = require("@lmb/http")
  local res = http:fetch(url)
  return res:text()
end

return http_get
