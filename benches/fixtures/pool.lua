local co = require("@lmb/coroutine")

-- Simulate work with a small computation
local function work()
    local sum = 0
    for i = 1, 100 do
        sum = sum + i
    end
    return sum
end

-- Create multiple coroutines to simulate concurrent requests
local threads = {}
for i = 1, 10 do
    threads[i] = coroutine.create(function()
        return work()
    end)
end

-- Run all concurrently
local results = co.join_all(threads)

-- Verify all completed successfully
local all_ok = true
for _, result in ipairs(results) do
    if result ~= 5050 then
        all_ok = false
        break
    end
end

return all_ok
