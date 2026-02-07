-- Test sleep_ms function
local start = os.clock()
sleep_ms(50)
local elapsed = (os.clock() - start) * 1000

-- Check that at least 40ms has passed (allowing some tolerance)
assert(elapsed >= 40, "sleep_ms did not wait long enough: " .. elapsed .. "ms")

return true
