function read_unicode()
  local m = require("@lmb")
  return m:read_unicode("*l")
end

return read_unicode
