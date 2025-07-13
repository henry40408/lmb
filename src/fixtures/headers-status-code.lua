local m = require("@lmb")
print(m.response)

local res = {}
res.status_code = 418 -- I'm a teapot
res.headers = { quantity = 1, whoami = "a teapot" }
m.response = res
print(m.response)

return "I'm a teapot."
