local m = require("@lmb").http
local res = m:fetch(io.read("*a") .. "/html")
return res:text()
