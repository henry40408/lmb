function base64()
  local crypto = require("@lmb/crypto")

  local text = " "

  local encoded = crypto.base64_encode(text)
  local expected = "IA=="
  assert(expected == encoded, "Expected '" .. expected .. "' but got '" .. encoded .. "'")
  local decoded = crypto.base64_decode(encoded)
  assert(text == decoded, "Expected '" .. text .. "' but got '" .. decoded .. "'")

  local crc32 = crypto.crc32(text)
  local expected = "e96ccf45"
  assert(expected == crc32, "Expected '" .. expected .. "' but got '" .. crc32 .. "'")
end

function hash()
  local crypto = require("@lmb/crypto")

  local text = " "

  local md5 = crypto.md5(text)
  local expected = "7215ee9c7d9dc229d2921a40e899ec5f"
  assert(expected == md5, "Expected '" .. expected .. "' but got '" .. md5 .. "'")

  local sha1 = crypto.sha1(text)
  local expected = "b858cb282617fb0956d960215c8e84d1ccf909c6"
  assert(expected == sha1, "Expected '" .. expected .. "' but got '" .. sha1 .. "'")

  local sha256 = crypto.sha256(text)
  local expected = "36a9e7f1c95b82ffb99743e0c5c4ce95d83c9a430aac59f84ef3cbfab6145068"
  assert(expected == sha256, "Expected '" .. expected .. "' but got '" .. sha256 .. "'")

  local sha384 = crypto.sha384(text)
  local expected = "588016eb10045dd85834d67d187d6b97858f38c58c690320c4a64e0c2f92eebd9f1bd74de256e8268815905159449566"
  assert(expected == sha384, "Expected '" .. expected .. "' but got '" .. sha384 .. "'")

  local sha512 = crypto.sha512(text)
  local expected =
    "f90ddd77e400dfe6a3fcf479b00b1ee29e7015c5bb8cd70f5f15b4886cc339275ff553fc8a053f8ddc7324f45168cffaf81f8c3ac93996f6536eef38e5e40768"
  assert(expected == sha512, "Expected '" .. expected .. "' but got '" .. sha512 .. "'")
end

function hmac_hash()
  local crypto = require("@lmb/crypto")

  local text = " "
  local key = "secret"

  local hmac_sha1 = crypto.hmac("sha1", text, key)
  local expected = "3fc26947ece0e3400c2216d2bcad669347e691ae"
  assert(expected == hmac_sha1, "Expected '" .. expected .. "' but got '" .. hmac_sha1 .. "'")

  local hmac_sha256 = crypto.hmac("sha256", text, key)
  local expected = "449cae45786ff49422f05eb94182fb6456b10db5c54f2342387168702e4f5197"
  assert(expected == hmac_sha256, "Expected '" .. expected .. "' but got '" .. hmac_sha256 .. "'")

  local hmac_sha384 = crypto.hmac("sha384", text, key)
  local expected = "82bb1e20f2d5c3ea86a7ecde4470923bfc7901b88a8154fe8e9ae9c326b822eb326b16517c72ee83294f376f6395a1ad"
  assert(expected == hmac_sha384, "Expected '" .. expected .. "' but got '" .. hmac_sha384 .. "'")

  local hmac_sha512 = crypto.hmac("sha512", text, key)
  local expected =
    "750198b84504923b67e3774963773255f4300aa7b3eaddd6a2eabb837d04c7f2e949d68faf22861fd1b560e66f7513eda5a47139a990f1ff5df90aac167fe4ca"
  assert(expected == hmac_sha512, "Expected '" .. expected .. "' but got '" .. hmac_sha512 .. "'")
end

function encrpyt_decrypt()
  local crypto = require("@lmb/crypto")

  local text = " "
  local key = "0123456701234567"
  local iv = "0123456701234567"

  local encrypted = crypto.encrypt("aes-cbc", text, key, iv)
  local expected = "b019fc0029f1ae88e96597dc0667e7c8"
  assert(expected == encrypted, "Expected '" .. expected .. "' but got '" .. encrypted .. "'")

  local decrypted = crypto.decrypt("aes-cbc", encrypted, key, iv)
  assert(text == decrypted, "Expected '" .. text .. "' but got '" .. decrypted .. "'")

  local key = "01234567"
  local iv = "01234567"
  local encrypted = crypto.encrypt("des-cbc", text, key, iv)
  local expected = "b865f90e7600d7ec"
  assert(expected == encrypted, "Expected '" .. expected .. "' but got '" .. encrypted .. "'")

  local decrypted = crypto.decrypt("des-cbc", encrypted, key, iv)
  assert(text == decrypted, "Expected '" .. text .. "' but got '" .. decrypted .. "'")

  local encrypted = crypto.encrypt("des-ecb", text, key)
  local expected = "7ab4f476973c70cc"
  assert(expected == encrypted, "Expected '" .. expected .. "' but got '" .. encrypted .. "'")

  local decrypted = crypto.decrypt("des-ecb", encrypted, key)
  assert(text == decrypted, "Expected '" .. text .. "' but got '" .. decrypted .. "'")
end

function crypto()
  base64()
  hash()
  hmac_hash()
  encrpyt_decrypt()
end

return crypto
