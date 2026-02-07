function join_all()
  local m = require("@lmb/coroutine")

  local a = coroutine.create(function()
    sleep_ms(100)
    return 1
  end)
  local b = coroutine.create(function()
    sleep_ms(100)
    return 2
  end)

  local a, b = table.unpack(m.join_all({ a, b }))
  assert(1 == a, "Expected 1, got " .. tostring(a))
  assert(2 == b, "Expected 2, got " .. tostring(b))
end

return join_all
