local crypto = require("@lmb/crypto")
local key = "12345678"
local iv = "87654321"
local plaintext = "testdata"
local encrypted = crypto.encrypt("des-cbc", plaintext, key, iv)
local decrypted = crypto.decrypt("des-cbc", encrypted, key, iv)
assert(decrypted == plaintext, "DES-CBC roundtrip failed")
return true
