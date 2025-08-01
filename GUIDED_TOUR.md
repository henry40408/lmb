# Guided Tour

This document provides a guided tour of the features and functionalities available in the project.

The following code blocks are examples of Lua code that can be executed within the context of this project. Each block is annotated with metadata that describes its purpose, input, and expected output. They are also tested to ensure correctness.

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

Luau is a Lua 5.1 language with gradual typing and ergonomic additions. For all packages and functions provided by Luau, please refer to the [Luau documentation](https://luau-lang.org/library).

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
function make_counter()
  local count = 1
  function increase()
    count = count + 1
    return count
  end
  return increase
end

return make_counter()
```

## Reading

To read input from the user, you can use the `io.read` function. Here's an example that reads a line of input and prints it:

```lua
--[[
--name = "Read all input"
--input = "Hello, Lua!\n你好, Lua!"
--assert_return = "Hello, Lua!\n你好, Lua!"
--]]
function read_all()
  local input = io.read("*a")
  return input
end

return read_all
```

Read a line of input and print it to the console. This example demonstrates how to handle user input in Lua.

```lua
--[[
--name = "Read line"
--input = "first line\nsecond line"
--assert_return = "first line"
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
--input = "Hello, Lua!"
--assert_return = "H"
--]]
function read_byte()
  local input = io.read(1)
  return input
end

return read_byte
```

Though Luau supports UTF-8, it doesn't provide a built-in way to read UTF-8 characters directly. Thus, we provide a simple function to read a UTF-8 character from the input. This function reads a byte and decodes it as a UTF-8 character.

```lua
--[[
--name = "Read UTF-8 character"
--input = "你好, Lua!"
--assert_return = "你"
--]]
function read_utf8_char()
  local m = require('@lmb')
  local input = m:read_unicode(1)
  return input
end

return read_utf8_char
```

## State

In Lua, you can maintain state using tables. Here's an example of how to create a simple state management system:

```lua
--[[
--name = "State"
--state = { a = 1, b = 2 }
--assert_return = 3
--]]
function state(ctx)
  return ctx.state.a + ctx.state.b
end

return state
```

## Store

> TODO

## Modules

### Coroutines

> TODO

### Cryptography

> TODO

### HTTP

> TODO

### JSON

> TODO
