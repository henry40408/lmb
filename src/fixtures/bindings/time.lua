local time = require("@lmb/time")

-- now_ms should return a positive integer
local ms = time.now_ms()
assert(type(ms) == "number", "now_ms should return a number")
assert(ms > 0, "now_ms should return a positive number")

-- now_ms should be greater than os.time() * 1000 (roughly)
local now_s = os.time()
assert(ms >= now_s * 1000, "now_ms should be >= os.time() * 1000")

-- parse with explicit format (strptime)
local ts = time.parse("2026-02-09", "%Y-%m-%d")
assert(type(ts) == "number", "parse should return a number")
assert(ts > 0, "parse should return a positive number")

local ts2 = time.parse("2026-02-09 12:00:00", "%Y-%m-%d %H:%M:%S")
assert(ts2 > ts, "timestamp with noon should be greater than midnight")

local epoch = time.parse("1970-01-01 00:00:00", "%Y-%m-%d %H:%M:%S")
assert(epoch == 0, "1970-01-01 00:00:00 should be epoch 0, got " .. tostring(epoch))

-- auto-detect: RFC 3339 / ISO 8601
local rfc3339 = time.parse("2026-02-09T12:00:00Z")
assert(rfc3339 == ts2, "RFC 3339 should match explicit parse, got " .. tostring(rfc3339))

local rfc3339_offset = time.parse("2026-02-09T20:00:00+08:00")
assert(rfc3339_offset == ts2, "RFC 3339 with +08:00 should match UTC noon, got " .. tostring(rfc3339_offset))

-- auto-detect: ISO 8601 date only
local iso_date = time.parse("2026-02-09")
assert(iso_date == ts, "ISO date should match explicit parse, got " .. tostring(iso_date))

-- auto-detect: ISO 8601 datetime without offset (treated as UTC)
local iso_dt = time.parse("2026-02-09 12:00:00")
assert(iso_dt == ts2, "ISO datetime should match explicit parse, got " .. tostring(iso_dt))

-- auto-detect: RFC 2822 / 1123
local rfc2822 = time.parse("Mon, 09 Feb 2026 12:00:00 +0000")
assert(rfc2822 == ts2, "RFC 2822 should match UTC noon, got " .. tostring(rfc2822))

-- auto-detect: asctime
local asctime = time.parse("Mon Feb  9 12:00:00 2026")
assert(asctime == ts2, "asctime should match UTC noon, got " .. tostring(asctime))

return true
