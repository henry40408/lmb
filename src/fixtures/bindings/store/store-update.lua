function store_update(ctx)
  ctx.store.a = 20 -- Initial value for 'a'
  ctx.store.b = 0  -- Reset 'b' for benchmark iterations

  -- a is fetched from the store
  -- b uses default value of 0 if not set above
  ctx.store:update({ "a", b = 0 }, function(values)
    values.a = values.a - 10
    values.b = values.b + 10
  end)

  assert(ctx.store.a == 10, "Expected a to be 10 after update, got " .. ctx.store.a)
  assert(ctx.store.b == 10, "Expected b to be 10 after update, got " .. ctx.store.b)

  local ok, err = pcall(function()
    ctx.store:update({ "a", "b" }, function(values)
      error("prevent a and b from being updated")
      values.a = values.a - 5
      values.b = values.b + 5
    end)
  end)
  assert(not ok, "Expected error when trying to update a and b")
  assert(string.find(tostring(err), "prevent a and b from being updated"), "Expected specific error message")

  assert(ctx.store.a == 10, "Expected a to remain 10 after failed update, got " .. ctx.store.a)
  assert(ctx.store.b == 10, "Expected b to remain 10 after failed update, got " .. ctx.store.b)
end

return store_update
