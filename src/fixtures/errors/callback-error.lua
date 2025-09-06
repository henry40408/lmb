function f()
  local json = require("@lmb/json")
  return json.decode("{")
end

return f
