function read_unicode()
  local lmb = require("@lmb")
  return lmb:read_unicode("*l")
end

return read_unicode
