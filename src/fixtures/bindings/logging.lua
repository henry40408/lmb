function test_logging()
  local log = require("@lmb/logging")

  -- Test all 5 log levels with a simple string
  log.error("error message")
  log.warn("warn message")
  log.info("info message")
  log.debug("debug message")
  log.trace("trace message")

  -- Test variadic arguments (tab-separated like print())
  log.info("hello", "world", 42)

  -- Test table serialization to JSON
  log.info("table", { key = "value", nested = { a = 1 } })

  -- Test nil argument
  log.info("nil value:", nil)

  -- Test no arguments
  log.info()

  -- Test numbers
  log.info(1, 2.5, 3)

  -- Test boolean
  log.info(true, false)

  -- Test mixed types
  log.info("mixed", 42, true, nil, { a = 1 })
end

return test_logging
