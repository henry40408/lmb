function serve_base64(ctx)
  local request = ctx.request
  local crypto = require("@lmb/crypto")
  return {
    is_base64_encoded = true,
    body = crypto.base64_encode("hello world"),
  }
end

return serve_base64
