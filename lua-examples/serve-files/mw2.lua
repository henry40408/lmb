--[[
--description = "The SECOND Lua script to log request."
--]]
local m = require("@lmb")
print(">", m.json:encode(m.request))
local res = m:next()
print("<", m.json:encode(res))
return res
