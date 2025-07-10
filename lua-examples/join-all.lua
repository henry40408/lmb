--[[
--description = "Join multiple coroutines and wait for all to finish."
--]]
local m = require("@lmb")
local joined = m.coroutine:join_all({
  coroutine.create(function()
    print("delay 2 seconds in coroutine 1")
    m.http:fetch("https://httpbingo.org/delay/2")
    print("end coroutine 1")
    return 1
  end),
  coroutine.create(function()
    print("delay 1 second in coroutine 2")
    m.http:fetch("https://httpbingo.org/delay/1")
    print("end coroutine 2")
    return 2
  end),
})
assert(joined[1] == 1, "Expected return value from coroutine 1")
assert(joined[2] == 2, "Expected return value from coroutine 2")
