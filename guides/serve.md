# Serve multiple files

Both `lmb eval` and `lmb serve` accept multiple files. However, when using `lmb eval`, scripts will be evaluated concurrently. When using `lmb serve`, the order matters, and only the last file will be used to handle HTTP requests. The rest of the scripts will be treated as middlewares. For example:

```lua
-- mw1.lua
local m = require('@lmb')
return io.read('*a') -- return request body

-- mw2.lua
local m = require('@lmb')
local json = require('@lmb/json')
print('>', json:encode(m.request))
local res = m:next() -- call mw1.lua
print('<', json:encode(res))
return res

-- last.lua
local m = require('@lmb')
return m:next() -- call mw2.lua
```

Run `lmb serve` with the above files:

```
$ lmb serve --bind 127.0.0.1:3000 --file mw1.lua --file mw2.lua --file last.lua
```

On the other terminal, send a request to the server:

```
$ curl --data-raw 1 http://localhost:3000
```

You will see the following output:

```
> {"method":"GET","url":"/","headers":{"host":"localhost:3000","user-agent":"curl/7.81.0","accept":"*/*"}}
< 1
```

The following diagram shows the flow of the request and response:

```
+--------+ --(request)-> +----------+ ---(next)-> +---------+ ---(next)-> +---------+
| client |               | last.lua |             | mw2.lua |             | mw1.lua |
+--------+ <-(response)- +----------+ <-(return)- +---------+ <-(return)- +---------+
```

When the request arrives, `last.lua` is the first script to be evaluated, which then calls `mw2.lua` with `next()`. `mw2.lua` is the second script to be evaluated, which then calls `mw1.lua` with `next()` as well. `mw1.lua` is the last script to be evaluated, which then returns the request body.

After `mw1.lua` is evaluated, the process reverses. `mw2.lua` receives the response from `mw1.lua`, and then it returns the response to `last.lua`. Finally, `last.lua` returns the response to the client.
