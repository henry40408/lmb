local crypto = require("@lmb/crypto")
-- Test that hash functions return non-empty hex strings
local data = "test data"
assert(#crypto.crc32(data) > 0, "crc32 failed")
assert(#crypto.md5(data) == 32, "md5 failed")
assert(#crypto.sha1(data) == 40, "sha1 failed")
assert(#crypto.sha256(data) == 64, "sha256 failed")
assert(#crypto.sha384(data) == 96, "sha384 failed")
assert(#crypto.sha512(data) == 128, "sha512 failed")
return true
