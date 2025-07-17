local logging = require("@lmb").logging

logging:log("log") -- simple string
logging:log("message", { a = 1, b = 2 }) -- string with table
logging:log({ 1, 2, 3 }) -- table with numbers
logging:log({ bool = true, ["nil"] = nil, num = 1, str = "string" }) -- table with mixed types
logging:log(true, 1, "string", nil) -- multiple arguments of different types

logging:log("log")
logging:error("error")
logging:warn("warn")
logging:info("info")
logging:debug("debug")
logging:trace("trace")
