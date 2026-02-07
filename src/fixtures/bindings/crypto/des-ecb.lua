local crypto = require("@lmb/crypto")
local key = "12345678"
local plaintext = "testdata"
local encrypted = crypto.encrypt("des-ecb", plaintext, key)
local decrypted = crypto.decrypt("des-ecb", encrypted, key)
assert(decrypted == plaintext, "DES-ECB roundtrip failed")
return true
