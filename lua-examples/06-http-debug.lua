local json = require("json")

local m = require("@lmb")

print("request")
print(string.format("%s %s", m.state.request["method"], m.state.request["path"]))
for key, values in pairs(m.state.request["headers"]) do
	for _, value in pairs(values) do
		print(string.format("%s = %s", key, value))
	end
end

m.state.status_code = 418
m.state.headers = {
	["content-type"] = "application/json",
	["x-appearence"] = { "material=pottery", "culture=Chinese" },
	["x-i-am"] = "a teapot",
}

return json.encode({ bool = true, num = 1, str = "str" })
