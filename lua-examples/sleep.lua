--[[
--description = "Schedule asynchronous tasks."
--]]
local async = require("@lmb/async")
local json = require("@lmb/json")
local tasks = {}
for i = 3, 1, -1 do
	print("sleep for", i, "seconds")
	table.insert(tasks, async:sleep_async(i))
end
local joined = async:join_all(tasks)
print("sleep joined", json:encode(joined))
