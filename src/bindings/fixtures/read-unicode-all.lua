function read_unicode()
  local lmb = require("@lmb")
  return lmb:read_unicode("*a")
end

return read_unicode
