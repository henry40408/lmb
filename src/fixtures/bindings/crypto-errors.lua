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

-- Test wrong AES-128 key length
ok, err = pcall(function()
    crypto.encrypt("aes-cbc", "data", "shortkey", "abcdefghijklmnop")
end)
assert(not ok and string.find(tostring(err), "AES%-128 key"), "Expected error for wrong AES-128 key length")

-- Test wrong AES-128 IV length
ok, err = pcall(function()
    crypto.encrypt("aes-cbc", "data", "1234567890123456", "shortiv")
end)
assert(not ok and string.find(tostring(err), "AES%-128 IV"), "Expected error for wrong AES-128 IV length")

-- Test wrong DES key length (des-cbc)
ok, err = pcall(function()
    crypto.encrypt("des-cbc", "data", "short", "87654321")
end)
assert(not ok and string.find(tostring(err), "DES key"), "Expected error for wrong DES key length (des-cbc)")

-- Test wrong DES IV length (des-cbc)
ok, err = pcall(function()
    crypto.encrypt("des-cbc", "data", "12345678", "short")
end)
assert(not ok and string.find(tostring(err), "DES IV"), "Expected error for wrong DES IV length (des-cbc)")

-- Test wrong DES key length (des-ecb)
ok, err = pcall(function()
    crypto.encrypt("des-ecb", "data", "short")
end)
assert(not ok and string.find(tostring(err), "DES key"), "Expected error for wrong DES key length (des-ecb)")

-- Test wrong key length in decrypt paths
ok, err = pcall(function()
    crypto.decrypt("aes-cbc", "00", "shortkey", "abcdefghijklmnop")
end)
assert(not ok and string.find(tostring(err), "AES%-128 key"), "Expected error for wrong AES-128 key length on decrypt")

ok, err = pcall(function()
    crypto.decrypt("aes-cbc", "00", "1234567890123456", "shortiv")
end)
assert(not ok and string.find(tostring(err), "AES%-128 IV"), "Expected error for wrong AES-128 IV length on decrypt")

ok, err = pcall(function()
    crypto.decrypt("des-cbc", "00", "short", "87654321")
end)
assert(not ok and string.find(tostring(err), "DES key"), "Expected error for wrong DES key length on decrypt (des-cbc)")

ok, err = pcall(function()
    crypto.decrypt("des-cbc", "00", "12345678", "short")
end)
assert(not ok and string.find(tostring(err), "DES IV"), "Expected error for wrong DES IV length on decrypt (des-cbc)")

ok, err = pcall(function()
    crypto.decrypt("des-ecb", "00", "short")
end)
assert(not ok and string.find(tostring(err), "DES key"), "Expected error for wrong DES key length on decrypt (des-ecb)")

return true
