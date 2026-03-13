local time = require("@lmb/time")

-- now_ms should return a positive integer
local ms = time.now_ms()
assert(type(ms) == "number", "now_ms should return a number")
assert(ms > 0, "now_ms should return a positive number")

-- now_ms should be greater than os.time() * 1000 (roughly)
local now_s = os.time()
assert(ms >= now_s * 1000, "now_ms should be >= os.time() * 1000")

-- parse should convert date strings to Unix timestamps
local ts = time.parse("2026-02-09", "%Y-%m-%d")
assert(type(ts) == "number", "parse should return a number")
assert(ts > 0, "parse should return a positive number")

-- parse with time component
local ts2 = time.parse("2026-02-09 12:00:00", "%Y-%m-%d %H:%M:%S")
assert(ts2 > ts, "timestamp with noon should be greater than midnight")

-- parse with known value: 1970-01-01 should be 0
local epoch = time.parse("1970-01-01 00:00:00", "%Y-%m-%d %H:%M:%S")
assert(epoch == 0, "1970-01-01 00:00:00 should be epoch 0, got " .. tostring(epoch))

return true
