local json = require("@lmb").json

local decoded = json:decode('{"bool":true,"num":2,"str":"hello"}')
assert(true == decoded.bool, "Expect true")
assert(2 == decoded.num, "Expect 2")
assert("hello" == decoded.str, 'Expect "hello"')

local encoded = json:encode({ bool = true, num = 2, str = "hello" })
assert('{"bool":true,"num":2,"str":"hello"}' == encoded, "Expect encoded JSON")

-- https://github.com/rxi/json.lua/issues/19
local nested = '{"a":[{}]}'
assert(nested == json:encode(json:decode(nested)), "Expect encode on decoded nested JSON")
