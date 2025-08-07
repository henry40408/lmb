function all_settled()
  local m = require('@lmb/coroutine')
  local a = coroutine.create(function()
    return true
  end)
  local b = coroutine.create(function()
    error('An error occurred')
  end)
  local settled = m.all_settled({ a, b })
  assert(#settled == 2, 'Expected 2 settled coroutines')

  assert(settled[1].status == 'fulfilled', 'First coroutine should be fulfilled, got ' .. settled[1].status)
  assert(settled[1].value == true, 'First coroutine should be fulfilled, got ' .. tostring(settled[1].value))

  assert(settled[2].status == 'rejected', 'Second coroutine should be rejected, got ' .. settled[2].status)
  -- Error message contains stack backtrace when it is passed through Rust and Lua,
  -- so we only check that it contains the expected message.
  -- ref: https://github.com/mlua-rs/mlua/issues/263
  assert(string.find(settled[2].reason, 'An error occurred'), 'Second coroutine should be rejected, got ' .. settled[2].reason)
end

return all_settled
