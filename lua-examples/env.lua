--[[
--description = "Print environment variable."
--]]
local m = require("@lmb")
print(m:get_env("PORT"))
