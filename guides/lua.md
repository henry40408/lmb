# Lua Guide

## Lua version

## Language Variant and Version

Lmb currently uses GopherLua.

```lua
assert('Lua 5.1' == _G._VERSION, 'expect ' .. _G._VERSION) -- Lua version
```

GopherLua is a Lua 5.1 (+goto statement in Lua 5.2) VM and compiler written in Go.

## Hello, world

First things first: Hello, World!

Save the following file as "hello.lua".

```lua
print('hello, world!')
```

Run the script:

```sh
$ lmb eval --file hello.lua
hello, world!
```

## I/O library

For security, the original `io` library is removed. However, because it's common to print and read something in daily use, Lmb implements the following functions/methods:

```lua
local io = require('io')

-- https://www.lua.org/manual/5.1/manual.html#pdf-print
print('hello, world!')

-- https://www.lua.org/manual/5.1/manual.html#pdf-io.stdout
io.write('standard output')

-- https://www.lua.org/manual/5.1/manual.html#pdf-io.read
assert(io.read('*L') == nil, 'expect nil for *L')
assert(io.read('*a') == nil, 'expect nil for *a')
assert(io.read('*l') == nil, 'expect nil for *l')
assert(io.read('*n') == nil, 'expect nil for *n')

local _, err = pcall(function() return io.read('*x') end)
assert(tostring(err):find('unsupported string format'))

-- https://www.lua.org/manual/5.1/manual.html#pdf-io.stderr
io.stderr:write('standard error')

-- output: standard output
```

```lua
-- input: 世界
return require('io').read(3)
-- output: 世
```

```lua
-- input: foobar
return require('io').read('*a')
-- output: foobar
```

```lua
-- input: 1949
return require('io').read('*n') ^ 2
-- output: 3798601
```

```lua
-- input: foo\nbar
return require('io').read('*l')
-- output: foo
```

```lua
-- input: foo\nbar
return require('io').read('*L')
-- output: foo\n
```

## Store

Lmb supports a key-value store backed by SQLite. The data can be read, written, and updated using the following APIs:

```lua
local m = require('@lmb')

-- read value from store
assert(not m.store['a'])

-- write value to store
m.store['a'] = 1
assert(m.store['a'] == 1)

-- update values in a transaction
function transfer(amount)
  m.store:update(function(store)
    if store['alice'] < amount then
      error('insufficient fund')
    end
    store['alice'] = store['alice'] - amount
    store['bob'] = store['bob'] + amount
  end)
end

-- failure scenario
m.store['alice'] = 50
m.store['bob'] = 50
local _, err = pcall(function()
  transfer(100)
end)
assert(tostring(err):find('insufficient fund')) -- error occurred
assert(m.store['alice'] == 50) -- as is
assert(m.store['bob'] == 50) -- as is

-- success scenario
m.store['alice'] = 100
m.store['bob'] = 0
local _, err = pcall(function()
  transfer(100)
end)
assert(not err) -- no error occurred
assert(m.store['alice'] == 0) -- withdrawn
assert(m.store['bob'] == 100) -- deposited
```
