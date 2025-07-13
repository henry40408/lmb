local m = require("@lmb").http
local res = m:fetch(io.read("*a") .. "/headers", { headers = { a = "b", ["user-agent"] = "agent/1.0" } })
return res:text()
