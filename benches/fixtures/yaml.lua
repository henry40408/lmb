local yaml = require("@lmb/yaml")

local data = {
    name = "benchmark",
    version = 1,
    enabled = true,
    tags = {"lua", "yaml", "benchmark"},
    config = {
        timeout = 30,
        retries = 3
    }
}

local encoded = yaml.encode(data)
local decoded = yaml.decode(encoded)

return decoded.name == "benchmark"
