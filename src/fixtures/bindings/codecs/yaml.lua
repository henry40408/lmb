function yaml()
  local m = require("@lmb/yaml")
  assert(m.encode({ key = "value" }) == "key: value\n", "YAML encoding failed")
  assert(m.decode("key: value").key == "value", "YAML decoding failed")
end

return yaml
