--[[
--description = "The SECOND Lua script to log request."
--]]
local m = require("@lmb")
local json = require("@lmb/json")
print(">", json:encode(m.request))
local res = m:next()
print("<", json:encode(res))
return res
