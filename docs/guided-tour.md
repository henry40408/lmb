# Guided tour

This document provides a guided tour of the features and functionalities available in the project.

The following code blocks are examples of Lua code that can be executed within the context of this project. Each block is annotated with metadata that describes its purpose, input, and expected output.

They are also tested to ensure correctness.

## Language variant and version

Lmb currently uses Luau from Roblox.

```lua
--[[
--name = "Luau Version"
--]]
function luau_version()
  local version = "0.706"
  assert(_G._VERSION == "Luau " .. version, "Expected Luau version " .. version .. ", but got " .. _G._VERSION)
end

return luau_version
```

Luau is a Lua 5.1 language with gradual typing and ergonomic additions. Sandbox is enabled for better security. For all packages and functions provided by Luau, please refer to the [Luau documentation](https://luau-lang.org/library).

## Hello, world!

First thing first, let's start with a simple "Hello, world!" example. This will demonstrate the basic structure of a Lua module and how to execute it.

```lua
--[[
--name = "Hello, world"
--]]
function hello()
  print("Hello, world!")
end

-- This returns the function so it can be called by the runner
return hello
```

## Expression

To reduce boilerplate, an expression can be evaluated for its result. This is useful for quick calculations or testing small snippets of code.

```lua
--[[
--name = "Shortcut"
--assert_return = "Hello, world!"
--]]
return "Hello, world!"
```

## Closure

In Lua, closures are a powerful feature that allows functions to capture their surrounding environment. Here's an example of how to create a closure:

```lua
--[[
--name = "Closure"
--assert_return = 2
--]]

-- function builder
function make_counter()
  -- local variable to hold the state
  local count = 1
  -- function that captures the state
  function increase()
    count = count + 1
    return count
  end
  return increase
end

return make_counter()
```

## Reading

According to [Luau documentation](https://luau.org/sandbox#library), `io` library is removed entirely from the sandbox. However, it's common to read input from the user in Lua scripts so we implement a custom input function. In this section, we will demonstrate how to read input using the `io.read` function.

Read all input and return it:

```lua
--[[
--name = "Read all input"
--assert_return = "Hello, Lua!\n你好, Lua!"
--input = "Hello, Lua!\n你好, Lua!"
--]]
function read_all()
  local input = io.read("*a")
  return input
end

return read_all
```

Read a line of input and return it:

```lua
--[[
--name = "Read line"
--assert_return = "first line"
--input = "first line\nsecond line"
--]]
function read_line()
  local input = io.read("*l")
  return input
end

return read_line
```

Read a byte from the input and return it. This example shows how to read a single byte from the input stream.

```lua
--[[
--name = "Read byte"
--assert_return = "H"
--input = "Hello, Lua!"
--]]
function read_byte()
  local input = io.read(1)
  return input
end

return read_byte
```

### Reading UTF-8 characters

Though [Luau supports UTF-8](https://luau.org/library#utf8-library), it doesn't provide a built-in way to read UTF-8 characters from the input. Thus, we provide a simple function to read a UTF-8 character from the input. This function reads a byte and decodes it as a UTF-8 character.

```lua
--[[
--name = "Read UTF-8 character"
--assert_return = "你"
--input = "你好, Lua!"
--]]
function read_utf8_char()
  local m = require('@lmb')
  local input = m:read_unicode(1)
  return input
end

return read_utf8_char
```

The function also accepts `*a` or `*l` as the first argument to read all characters or a line of characters, respectively, like the `io.read` function.

## State

In Lua, you can maintain state using tables. Here's an example of how to create a simple state management system:

```lua
--[[
--name = "State"
--assert_return = 3
--state = { a = 1, b = 2 }
--]]
function state(ctx)
  return ctx.state.a + ctx.state.b
end

return state
```

## Store

In this section, we demonstrate how to use a store to manage state across different parts of your application. The store allows you to update and retrieve values in a structured way.

- Value can be fetched or stored with `ctx.store`, which is a table-like object.
- Values can be updated using the `ctx.store:update` method, which takes a table of keys and optional default values and a function to modify the values. If the function returns an error, the update will not be applied because a transaction is used under the hood.

```lua
--[[
--name = "Store"
--store = true
--]]
function store(ctx)
  assert(not ctx.store.a, "Expected ctx.store.a to be nil")
  ctx.store.a = 20
  assert(20 == ctx.store.a, "Expected ctx.store.a to be 20, got " .. tostring(ctx.store.a))

  ctx.store:update({ "a", b = 0 }, function(values)
    assert(values.a == 20, "Expected values.a to be 20, got " .. values.a)
    assert(values.b == 0, "Expected values.b to be 0, got " .. values.b)

    values.a = values.a - 10
    values.b = values.b + 10
  end)

  assert(ctx.store.a == 10, "Expected ctx.store.a to be 10, got " .. tostring(ctx.store.a))
  assert(ctx.store.b == 10, "Expected ctx.store.b to be 10, got " .. tostring(ctx.store.b))

  local ok, err = pcall(function()
    ctx.store:update({ "a", "b" }, function(values)
      error("prevent a and b from being updated")
      values.a = values.a + 5
      values.b = values.b - 5
    end)
  end)
  assert(not ok, "Expected error when trying to update a and b")
  assert(string.find(tostring(err), "prevent a and b from being updated"), "Expected specific error message")
end

return store
```

### Difference between state and store

The main difference between state and store is that state should be considered ephemeral and is not persisted across runs, while store is persistent and can be used to store values that need to be accessed in later runs.

## Environment variables

Traditionally, developers retrieve environment variables using the [`os.getenv` function](https://www.lua.org/pil/22.2.html). In Luau, this function is unavailable when the sandbox is enabled for security reasons. Because environment variables are still commonly needed, we provide a safe alternative: the user must explicitly request access to each variable:

```bash
$ echo 'return require("@lmb"):getenv("FOO")' | FOO=bar lmb --allow-env FOO eval --file -
bar
```

```lua
--[[
--name = "Environment variable"
--assert_return = null
--]]
function getenv()
  local m = require("@lmb")
  -- since FOO is not allowed, this will return null
  return m:getenv("FOO")
end

return getenv
```

## Modules

### Coroutines

Coroutines are a powerful feature in Lua that allows you to pause and resume functions, enabling cooperative multitasking.

#### join_all

Here's an example of how to use coroutines to join multiple coroutines together:

```lua
--[[
--name = "Coroutines - join all"
--timeout = 210
--]]
function join_all()
  local m = require('@lmb/coroutine')
  local a = coroutine.create(function()
    sleep_ms(100)
    return 100
  end)
  local b = coroutine.create(function()
    sleep_ms(200)
    return 200
  end)
  local values = m.join_all({ a, b })
  assert(values[1] == 100, "Expected first coroutine to return 100")
  assert(values[2] == 200, "Expected second coroutine to return 200")
end

return join_all
```

#### all_settled

The `all_settled` function waits for all coroutines to complete and returns an array of result objects. Unlike `join_all`, it does not fail if any coroutine fails. Each result object has:
- `status`: "fulfilled" or "rejected"
- `value`: The return value (if fulfilled)
- `reason`: The error (if rejected)

```lua
--[[
--name = "Coroutines - all_settled"
--timeout = 110
--]]
function all_settled()
  local m = require('@lmb/coroutine')
  local a = coroutine.create(function()
    return 100
  end)
  local b = coroutine.create(function()
    error("intentional error")
  end)
  local results = m.all_settled({ a, b })

  assert(results[1].status == "fulfilled", "Expected first coroutine to be fulfilled")
  assert(results[1].value == 100, "Expected first coroutine value to be 100")

  assert(results[2].status == "rejected", "Expected second coroutine to be rejected")
  assert(results[2].reason ~= nil, "Expected second coroutine to have a reason")
end

return all_settled
```

#### race

In this example, we demonstrate how to use coroutines to race multiple coroutines against each other and return the result of the first one that finishes.

```lua
--[[
--name = "Coroutines - race"
--timeout = 10
--]]
function race()
  local m = require('@lmb/coroutine')
  local a = coroutine.create(function()
    sleep_ms(1)
    return 100
  end)
  local b = coroutine.create(function()
    sleep_ms(2)
    return 200
  end)
  local actual = m.race({ a, b })
  assert(100 == actual, "Expected the first coroutine to finish first, got " .. actual)
end

return race
```

### Logging

The `@lmb/logging` module provides standard log-level functions that integrate with the Rust `tracing` framework. Each method accepts variadic arguments, which are converted to strings and joined with tab characters (matching Lua `print()` convention). Tables are automatically serialized to JSON.

Available log levels: `error`, `warn`, `info`, `debug`, `trace`.

```lua
--[[
--name = "Logging"
--]]
function logging()
  local log = require("@lmb/logging")

  -- Log at different levels
  log.error("something went wrong")
  log.warn("deprecated feature used")
  log.info("server started", "port", 8080)
  log.debug("processing request", { method = "GET", path = "/" })
  log.trace("detailed step")

  -- Tables are serialized to JSON
  log.info("config", { host = "localhost", port = 3000 })
end

return logging
```

Use `RUST_LOG=lmb::lua=debug` to control the log level for Lua scripts independently from the rest of the application.

### Cryptography

In this section, we demonstrate various cryptographic functions such as encoding, hashing, HMAC, and encryption/decryption. These functions are essential for secure data handling and communication.

```lua
--[[
--name = "Cryptography"
--]]

-- Base64 encoding and decoding
function base64()
  local crypto = require("@lmb/crypto")

  local text = " "

  local encoded = crypto.base64_encode(text)
  local expected = "IA=="
  assert(expected == encoded, "Expected '" .. expected .. "' but got '" .. encoded .. "'")
  local decoded = crypto.base64_decode(encoded)
  assert(text == decoded, "Expected '" .. text .. "' but got '" .. decoded .. "'")
end

-- Hashing functions
function hash()
  local crypto = require("@lmb/crypto")

  local text = " "

  -- CRC32 checksum
  local crc32 = crypto.crc32(text)
  local expected = "e96ccf45"
  assert(expected == crc32, "Expected '" .. expected .. "' but got '" .. crc32 .. "'")

  -- MD5
  -- NOTE: The MD5 algorithm is considered cryptographically broken and unsuitable for further use.
  local md5 = crypto.md5(text)
  local expected = "7215ee9c7d9dc229d2921a40e899ec5f"
  assert(expected == md5, "Expected '" .. expected .. "' but got '" .. md5 .. "'")

  -- SHA1
  local sha1 = crypto.sha1(text)
  local expected = "b858cb282617fb0956d960215c8e84d1ccf909c6"
  assert(expected == sha1, "Expected '" .. expected .. "' but got '" .. sha1 .. "'")

  -- SHA256
  local sha256 = crypto.sha256(text)
  local expected = "36a9e7f1c95b82ffb99743e0c5c4ce95d83c9a430aac59f84ef3cbfab6145068"
  assert(expected == sha256, "Expected '" .. expected .. "' but got '" .. sha256 .. "'")

  -- SHA384
  local sha384 = crypto.sha384(text)
  local expected = "588016eb10045dd85834d67d187d6b97858f38c58c690320c4a64e0c2f92eebd9f1bd74de256e8268815905159449566"
  assert(expected == sha384, "Expected '" .. expected .. "' but got '" .. sha384 .. "'")

  -- SHA512
  local sha512 = crypto.sha512(text)
  local expected = "f90ddd77e400dfe6a3fcf479b00b1ee29e7015c5bb8cd70f5f15b4886cc339275ff553fc8a053f8ddc7324f45168cffaf81f8c3ac93996f6536eef38e5e40768"
  assert(expected == sha512, "Expected '" .. expected .. "' but got '" .. sha512 .. "'")
end

-- HMAC with different hash algorithms
function hmac_hash()
  local crypto = require("@lmb/crypto")

  local text = " "
  local key = "secret"

  -- HMAC with SHA1
  local hmac_sha1 = crypto.hmac("sha1", text, key)
  local expected = "3fc26947ece0e3400c2216d2bcad669347e691ae"
  assert(expected == hmac_sha1, "Expected '" .. expected .. "' but got '" .. hmac_sha1 .. "'")

  -- HMAC with SHA256
  local hmac_sha256 = crypto.hmac("sha256", text, key)
  local expected = "449cae45786ff49422f05eb94182fb6456b10db5c54f2342387168702e4f5197"
  assert(expected == hmac_sha256, "Expected '" .. expected .. "' but got '" .. hmac_sha256 .. "'")

  -- HMAC with SHA384
  local hmac_sha384 = crypto.hmac("sha384", text, key)
  local expected = "82bb1e20f2d5c3ea86a7ecde4470923bfc7901b88a8154fe8e9ae9c326b822eb326b16517c72ee83294f376f6395a1ad"
  assert(expected == hmac_sha384, "Expected '" .. expected .. "' but got '" .. hmac_sha384 .. "'")

  -- HMAC with SHA512
  local hmac_sha512 = crypto.hmac("sha512", text, key)
  local expected = "750198b84504923b67e3774963773255f4300aa7b3eaddd6a2eabb837d04c7f2e949d68faf22861fd1b560e66f7513eda5a47139a990f1ff5df90aac167fe4ca"
  assert(expected == hmac_sha512, "Expected '" .. expected .. "' but got '" .. hmac_sha512 .. "'")
end

-- Encryption and decryption
function encrypt_decrypt()
  local crypto = require("@lmb/crypto")

  local text = " "
  local key = "0123456701234567"
  local iv = "0123456701234567"

  -- AES-CBC
  local encrypted = crypto.encrypt("aes-cbc", text, key, iv)
  local expected = "b019fc0029f1ae88e96597dc0667e7c8"
  assert(expected == encrypted, "Expected '" .. expected .. "' but got '" .. encrypted .. "'")

  local decrypted = crypto.decrypt("aes-cbc", encrypted, key, iv)
  assert(text == decrypted, "Expected '" .. text .. "' but got '" .. decrypted .. "'")

  -- DES-CBC
  local key = "01234567"
  local iv = "01234567"
  local encrypted = crypto.encrypt("des-cbc", text, key, iv)
  local expected = "b865f90e7600d7ec"
  assert(expected == encrypted, "Expected '" .. expected .. "' but got '" .. encrypted .. "'")

  local decrypted = crypto.decrypt("des-cbc", encrypted, key, iv)
  assert(text == decrypted, "Expected '" .. text .. "' but got '" .. decrypted .. "'")

  -- DES-ECB
  local encrypted = crypto.encrypt("des-ecb", text, key)
  local expected = "7ab4f476973c70cc"
  assert(expected == encrypted, "Expected '" .. expected .. "' but got '" .. encrypted .. "'")

  local decrypted = crypto.decrypt("des-ecb", encrypted, key)
  assert(text == decrypted, "Expected '" .. text .. "' but got '" .. decrypted .. "'")
end

function crypto()
  base64()
  hash()
  hmac_hash()
  encrypt_decrypt()
end

return crypto
```

### HTTP

In this section, we demonstrate how to make HTTP requests using the `@lmb/http` module. This module provides a simple interface for making GET and POST requests, handling responses, and parsing JSON data. The API is similar to the [`fetch` API](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API/Using_Fetch) in JavaScript.

```lua
--[[
--name = "HTTP"
--# NOTE: The url is a placeholder; it will be overridden by the test harness.
--state = { url = "http://localhost:8080" }
--]]
function http_get(ctx)
  local http = require("@lmb/http")

  local res = http:fetch(ctx.state.url .. "/get")
  assert(200 == res.status, "Expected status 200, got " .. res.status)

  local body = res:json()
  assert(1 == body.a, "Expected body.a to be 1, got " .. body.a)
end

return http_get
```

### Parse path

The `parse_path` function extracts named parameters from a URL path by matching it against a pattern. It returns a table of parameter key-value pairs on match, or `nil` if the path does not match.

Pattern syntax uses `{name}` for named parameters and `{*name}` for catch-all parameters.

```lua
--[[
--name = "Parse path"
--]]
function parse_path()
  local http = require("@lmb/http")

  -- Extract a single parameter
  local params = http.parse_path("/users/42", "/users/{id}")
  assert(params.id == "42", "Expected id to be '42'")

  -- Extract multiple parameters
  params = http.parse_path("/users/42/posts/99", "/users/{user_id}/posts/{post_id}")
  assert(params.user_id == "42", "Expected user_id to be '42'")
  assert(params.post_id == "99", "Expected post_id to be '99'")

  -- Returns nil when the path does not match
  params = http.parse_path("/other/path", "/users/{id}")
  assert(params == nil, "Expected nil for non-matching path")

  -- Catch-all parameter
  params = http.parse_path("/files/docs/readme.md", "/files/{*rest}")
  assert(params.rest == "docs/readme.md", "Expected rest to be 'docs/readme.md'")
end

return parse_path
```

## Encoding and decoding

### JSON

In this section, we demonstrate how to work with JSON data in Lua using the `@lmb/json` module. This module provides functions for encoding and decoding JSON data, making it easy to work with structured data.

```lua
--[[
--name = "JSON"
--assert_return = "{\"a\":2}"
--input = "{\"a\":1}"
--]]
function json_decode()
  local json = require("@lmb/json")
  local decoded = json.decode(io.read("*a"))
  decoded.a = decoded.a + 1
  return json.encode(decoded)
end

return json_decode
```

#### JSON path

[JSON path](https://goessner.net/articles/JsonPath/) is a powerful way to query and manipulate JSON data. In this section, we demonstrate how to use JSON path to extract values from a JSON object.

```lua
--[[
--name = "JSON Path"
--assert_return = "[\"foo\",\"bar\",\"baz\"]"
--input = "[{\"value\":1,\"name\":\"foo\"},{\"value\":2,\"name\":\"bar\"},{\"value\":3,\"name\":\"baz\"}]"
--]]
function json_path()
  local json = require('@lmb/json')
  local json_path = require('@lmb/json-path')
  return json.encode(json_path.query('$[*].name', json.decode(io.read("*a"))))
end

return json_path
```

### TOML

[TOML](https://toml.io/en/) (Tom's Obvious, Minimal Language) is a data serialization format that is easy to read and write. In this section, we demonstrate how to work with TOML data in Lua using the `@lmb/toml` module.

```lua
--[[
--name = "TOML"
--assert_return = "a = 2\nb = 3\n"
--input = "a = 1\nb = 2\n"
--]]
function toml_decode()
  local toml = require("@lmb/toml")
  local decoded = toml.decode(io.read("*a"))
  decoded.a = decoded.a + 1
  decoded.b = decoded.b + 1
  return toml.encode(decoded)
end

return toml_decode
```

### YAML

[YAML](https://yaml.org) (YAML Ain't Markup Language) is a human-readable data serialization format. In this section, we demonstrate how to work with YAML data in Lua using the `@lmb/yaml` module.

```lua
--[[
--name = "YAML"
--assert_return = "a: 2\nb: 3\n"
--input = "a: 1\nb: 2\n"
--]]
function yaml_decode()
  local yaml = require("@lmb/yaml")
  local decoded = yaml.decode(io.read("*a"))
  decoded.a = decoded.a + 1
  decoded.b = decoded.b + 1
  return yaml.encode(decoded)
end

return yaml_decode
```

## Handle HTTP requests with Lua scripts

The following example shows a curl command and a Lua script named `handle_request.lua`.

```lua
--[[
--name = "Handle requests with Lua script"
--input = '{"a":1}'
--curl = "curl -X POST -H 'x-api-key: api-key' --data '{\"a\":1}' http://localhost:3000/a/b/c"
--]]
function handle_request(ctx)
  local request = ctx.request
  assert("POST" == request.method, "Expected POST method")
  assert("/a/b/c" == request.path, "Expected path /a/b/c")
  assert("api-key" == request.headers["x-api-key"], "Expected x-api-key header")
  assert('{"a":1}' == io.read("*a"), 'Expected body {"a":1}')
  return {
    status_code = 201,
    headers = { ["content-type"] = "text/html", ["i-am"] = "teapot" },
    body = "<h1>I am a teapot</h1>",
  }
end

return handle_request
```

Run the following commands to handle requests with the Lua script:

```bash
$ lmb serve --file handle_request.lua &
$ curl -v -H 'x-api-key: api-key' --data '{"a":1}' http://localhost:3000/a/b/c
< HTTP/1.1 201 Created
< content-type: text/html
< i-am: teapot
< content-length: 22
<
<h1>I am a teapot</h1>
```

Response body can be base64 encoded by setting `is_base64_encoded` to true in the returned table. This is useful when the response body contains binary data.

```lua
--[[
--name = "Respond with base64 encoded body"
--curl = "curl http://localhost:3000"
--]]
function handle_request_base64(ctx)
  local request = ctx.request
  local crypto = require("@lmb/crypto")
  return {
    is_base64_encoded = true,
    body = crypto.base64_encode("hello world"),
  }
end

return handle_request_base64
```

Run the following commands to handle requests with the Lua script:

```bash
$ lmb serve --file handle_request_base64.lua &
$ curl -v http://localhost:3000
< HTTP/1.1 200 OK
< content-length: 11
<
hello world
```
