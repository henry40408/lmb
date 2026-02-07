function read_unicode()
  local m = require("@lmb")
  return m:read_unicode("*a")
end

return read_unicode
