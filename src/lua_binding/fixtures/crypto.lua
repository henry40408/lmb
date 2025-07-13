local crypto = require("@lmb").crypto

local input = " "

local hashed = crypto:sha256(input)
assert(hashed == "36a9e7f1c95b82ffb99743e0c5c4ce95d83c9a430aac59f84ef3cbfab6145068", "Unexpected SHA256 hash")

local hashed = crypto:hmac("sha256", input, "secret")
assert(hashed == "449cae45786ff49422f05eb94182fb6456b10db5c54f2342387168702e4f5197", "Unexpected SHA256-HMAC hash")

local key_iv = "0123456701234567"
local algo = "aes-cbc"

local encrypted = crypto:encrypt(input, algo, key_iv, key_iv)
assert(encrypted == "b019fc0029f1ae88e96597dc0667e7c8", "Unexpected AES-CBC encrypted result")

local decrypted = crypto:decrypt(encrypted, algo, key_iv, key_iv)
assert(decrypted == input, "Unexpected AES-CBC decrypted result")
