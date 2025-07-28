function store_update(ctx)
  ctx.store:update({ a = 20, b = 0 }, function(values)
    values.a = values.a - 10
    values.b = values.b + 10
  end)
  assert(ctx.store.a == 10, "Expected a to be 10 after update")
  assert(ctx.store.b == 10, "Expected b to be 10 after update")
end

return store_update
