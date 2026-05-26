local regex = require("@lmb/regex")

-- match: returns the first match
assert(regex.match("hello world", "\\w+") == "hello", "match should return first word")
-- match: returns nil when there is no match
assert(regex.match("xyz", "\\d+") == nil, "match should return nil when no match")

-- captures: returns capture groups (excluding the whole match)
local caps = regex.captures("2026-02-09", "(\\d{4})-(\\d{2})-(\\d{2})")
assert(type(caps) == "table", "captures should return a table")
assert(#caps == 3, "captures should return 3 groups, got " .. #caps)
assert(caps[1] == "2026", "group 1 should be 2026, got " .. tostring(caps[1]))
assert(caps[2] == "02", "group 2 should be 02, got " .. tostring(caps[2]))
assert(caps[3] == "09", "group 3 should be 09, got " .. tostring(caps[3]))
-- captures: returns nil when there is no match
assert(regex.captures("nope", "(\\d+)") == nil, "captures should return nil when no match")

-- find_all: returns every match
local all = regex.find_all("a1b2c3", "\\d+")
assert(#all == 3, "find_all should return 3 matches, got " .. #all)
assert(all[1] == "1" and all[2] == "2" and all[3] == "3", "find_all values")
-- find_all: returns an empty table when there are no matches
local none = regex.find_all("abc", "\\d+")
assert(#none == 0, "find_all should return empty table when no matches")

-- replace: replaces the first occurrence only
assert(regex.replace("foo bar baz", "\\s+", "-") == "foo-bar baz", "replace first only")
-- replace_all: replaces every occurrence
assert(regex.replace_all("a1b2c3", "\\d", "X") == "aXbXcX", "replace_all")

-- split: splits on the pattern, collapsing repeated separators
local parts = regex.split("a,b,,c", ",+")
assert(#parts == 3, "split should return 3 parts, got " .. #parts)
assert(parts[1] == "a" and parts[2] == "b" and parts[3] == "c", "split values")

-- is_match: boolean test
assert(regex.is_match("hello123", "\\d+") == true, "is_match should be true")
assert(regex.is_match("hello", "\\d+") == false, "is_match should be false")

-- invalid pattern raises an error
local ok = pcall(regex.is_match, "x", "(")
assert(not ok, "invalid pattern should raise an error")

return true
