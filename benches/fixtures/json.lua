function json()
  local json = require("@lmb/json")
  assert(json.encode({ key = "value" }) == '{"key":"value"}', "JSON encoding failed")
  assert(json.decode('{"key":"value"}').key == "value", "JSON decoding failed")
end

return json
