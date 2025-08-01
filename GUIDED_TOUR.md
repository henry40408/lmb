# Guided Tour

This document provides a guided tour of the features and functionalities available in the project.

The following code blocks are examples of Lua code that can be executed within the context of this project. Each block is annotated with metadata that describes its purpose, input, and expected output.

They are also tested to ensure correctness.

## Language Variant and Version

Lmb currently uses Luau from Roblox.

```lua
--[[
--name = "Luau Version"
--]]
function luau_version()
  local version = "0.682"
  assert(_G._VERSION == "Luau " .. version, "Expected Luau version " .. version .. ", but got " .. _G._VERSION)
end

return luau_version
```

Luau is a Lua 5.1 language with gradual typing and ergonomic additions. Sandbox is enabled for better security. For all packages and functions provided by Luau, please refer to the [Luau documentation](https://luau-lang.org/library).

## Hello, World!

First thing first, let's start with a simple "Hello, World!" example. This will demonstrate the basic structure of a Lua module and how to execute it.

```lua
--[[
--name = "Hello World"
--]]
function hello()
  print("Hello, World!")
end

-- This returns the function so it can be called by the runner
return hello
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

### Reading UTF-8 Characters

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

The function also accepts "*a" or "*l" as the first argument to read all characters or a line of characters, respectively, like the `io.read` function.

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
  assert(20 == ctx.store.a, "Expected ctx.store.a to be 20, got " .. ctx.store.a)

  ctx.store:update({ "a", b = 0 }, function(values)
    assert(values.a == 20, "Expected values.a to be 20, got " .. values.a)
    assert(values.b == 0, "Expected values.b to be 0, got " .. values.b)

    values.a = values.a - 10
    values.b = values.b + 10
  end)

  assert(ctx.store.a == 10, "Expected ctx.store.a to be 10, got " .. ctx.store.a)
  assert(ctx.store.b == 10, "Expected ctx.store.b to be 10, got " .. ctx.store.b)

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
  end)
  local b = coroutine.create(function()
    sleep_ms(200)
  end)
  m.join_all({ a, b })
end

return join_all
```

#### race

In this example, we demonstrate how to use coroutines to race multiple coroutines against each other and return the result of the first one that finishes.

```lua
--[[
--name = "Coroutines - race"
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

### Cryptography

> TODO

### HTTP

> TODO

### JSON

> TODO
