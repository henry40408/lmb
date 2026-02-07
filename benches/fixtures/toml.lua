local toml = require("@lmb/toml")

local data = {
    name = "benchmark",
    version = 1,
    enabled = true,
    config = {
        timeout = 30,
        retries = 3
    }
}

local encoded = toml.encode(data)
local decoded = toml.decode(encoded)

return decoded.name == "benchmark"
