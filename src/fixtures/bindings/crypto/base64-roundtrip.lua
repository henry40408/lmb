local crypto = require("@lmb/crypto")
local original = "Hello, World!"
local encoded = crypto.base64_encode(original)
local decoded = crypto.base64_decode(encoded)
assert(decoded == original, "base64 roundtrip failed")
return true
