local function test_crypto()
	local crypto = require("crypto")
	assert(crypto.sha256("lua") == "dc436b329f4da6c88b0b6ce79d829ac4fc33746b454604e2bcbad25a8e2985fe")
end

local function test_http()
	local lmb = require("@lmb")
	local base_url = lmb.state["url"] or "https://ip.me"

	local http = require("http")
	local res = http.get(base_url, {
		headers = { ["user-agent"] = "curl/0.0.0" },
	})
	local ip = res.body:gsub("%s+", "")
	assert(ip)
end

local function test_json()
	local json = require("json")
	assert(json.encode({ a = 1 }) == [[{"a":1}]])
	assert(json.decode([[{"a":1}]]).a == 1)
end

local function test_logger()
	local logger = require("logger")
	logger.debug("debug", { foo = "bar", baz = 1 })
	logger.info("info", { foo = "bar", baz = 1 })
	logger.warn("warn", { foo = "bar", baz = 1 })
	logger.error("error", { foo = "bar", baz = 1 })
end

local function test_regex()
	local re = require("re")
	local start, finish = re.find("hello world", "o w", 1, true)
	assert(start == 5 and finish == 7)
	assert(re.gsub("hello world", "world", "WORLD") == "hello WORLD")
	assert(re.match("hello world", "\\w+") == "hello")
end

local function test_url()
	local url = require("url")
	local parsed = url.parse("https://httpbingo.org/get")
	parsed.query = url.build_query_string({ a = "b" })
	assert(url.build(parsed) == "https://httpbingo.org/get?a=b")
end

local function main()
	test_crypto()
	test_http()
	test_json()
	test_logger()
	test_regex()
	test_url()
	return true
end

return main()
