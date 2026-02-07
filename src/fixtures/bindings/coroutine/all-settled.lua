function all_settled()
  local m = require("@lmb/coroutine")

  local a = coroutine.create(function()
    error("An error occurred")
  end)
  local b = coroutine.create(function()
    return true
  end)

  local settled = m.all_settled({ a, b })
  assert(#settled == 2)

  assert(settled[1].status == "rejected", "Expected first coroutine to be rejected, got " .. settled[1].status)
  local e = tostring(settled[1].reason)
  assert(string.find(e, "An error occurred"), "Expected first coroutine to have error reason, got " .. e)

  assert(settled[2].status == "fulfilled", "Expected second coroutine to be fulfilled, got " .. settled[2].status)
  assert(settled[2].value == true, "Expected second coroutine to have value, got " .. tostring(settled[2].value))
end

return all_settled
