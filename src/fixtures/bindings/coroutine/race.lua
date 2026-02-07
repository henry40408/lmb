function race()
  local m = require("@lmb/coroutine")

  local a = coroutine.create(function()
    sleep_ms(100)
    return 1
  end)
  local b = coroutine.create(function()
    sleep_ms(200)
    return 2
  end)

  local result = m.race({ a, b })
  assert(1 == result, "Expected 1, got " .. tostring(result))
end

return race
