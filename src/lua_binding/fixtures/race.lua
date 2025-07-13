local m = require("@lmb")
return m.coroutine:race({
  coroutine.create(function()
    m:sleep_ms(100)
    return 100
  end),
  coroutine.create(function()
    m:sleep_ms(200)
    return 200
  end),
})
