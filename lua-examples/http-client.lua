--[[
--description = "Send HTTP GET request."
--]]
local m = require("@lmb")
local res = m.http:fetch("https://httpbingo.org/get")
print(m.json:encode(res:json()))
