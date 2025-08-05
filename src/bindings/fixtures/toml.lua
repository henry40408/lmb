function toml()
  local m = require("@lmb/toml")
  assert(m.encode({ key = "value" }) == 'key = "value"\n', "TOML encoding failed")
  assert(m.decode('key = "value"').key == "value", "TOML decoding failed")
end

return toml
