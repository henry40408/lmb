function store_update(ctx)
  ctx.store:update({ a = 0, b = 0 }, function(values)
    values.a = values.a - 1
    values.b = values.b + 1
  end)
  assert(ctx.store.a + ctx.store.b == 0, "Expected a + b to be 0 after update")
end

return store_update
