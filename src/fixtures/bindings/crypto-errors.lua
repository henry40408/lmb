-- Test crypto error handling
local crypto = require("@lmb/crypto")

-- Test invalid base64 decode
local ok, err = pcall(function()
    crypto.base64_decode("!!!invalid-base64!!!")
end)
assert(not ok, "Expected error for invalid base64")

-- Test invalid hex in decrypt
ok, err = pcall(function()
    crypto.decrypt("aes-cbc", "not-hex-data", "1234567890123456", "abcdefghijklmnop")
end)
assert(not ok, "Expected error for invalid hex data")

-- Test unsupported cipher
ok, err = pcall(function()
    crypto.encrypt("unsupported-cipher", "data", "key", "iv")
end)
assert(not ok, "Expected error for unsupported cipher")

-- Test unsupported hmac algorithm
ok, err = pcall(function()
    crypto.hmac("unsupported", "data", "secret")
end)
assert(not ok, "Expected error for unsupported hmac algorithm")

-- Test missing IV for aes-cbc
ok, err = pcall(function()
    crypto.encrypt("aes-cbc", "data", "1234567890123456")
end)
assert(not ok, "Expected error for missing IV")

return true
