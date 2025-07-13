local m = require("@lmb").http
local res = m:fetch(io.read("*a") .. "/add", {
  method = "POST",
  body = "1+1",
})
return res:text()
