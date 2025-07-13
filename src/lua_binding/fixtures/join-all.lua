local m = require("@lmb")
local ms = assert(io.read("*n"))
m.coroutine:join_all({
  coroutine.create(function()
    m:sleep_ms(ms)
  end),
  coroutine.create(function()
    m:sleep_ms(ms)
  end),
})
