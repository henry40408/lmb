-- Regression test: concurrent fetch calls on the same http object
-- must not cause UserDataBorrowError.
function concurrent_fetch(ctx)
  local url = ctx.state
  local http = require("@lmb/http")

  local function fetch_a()
    local res = http:fetch(url .. "/a")
    return res:text()
  end

  local function fetch_b()
    local res = http:fetch(url .. "/b")
    return res:text()
  end

  local m = require("@lmb/coroutine")
  local results = m.join_all({
    coroutine.create(fetch_a),
    coroutine.create(fetch_b),
  })

  return results
end

return concurrent_fetch
