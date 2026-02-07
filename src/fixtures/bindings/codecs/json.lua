function json()
  local json = require("@lmb/json")
  assert(json.encode({ key = "value" }) == '{"key":"value"}', "JSON encoding failed")
  assert(json.decode('{"key":"value"}').key == "value", "JSON decoding failed")

  -- https://github.com/rxi/json.lua/issues/19
  local nested = '{"a":[{}]}'
  assert(nested == json.encode(json.decode(nested)), "Expect encode on decoded nested JSON")
end

return json
