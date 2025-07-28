function store_update(ctx)
  ctx.store:update({ "a", "b" }, function(values)
    values.a = values.a - 10
    values.b = values.b + 10
  end, { 20, 0 })
  assert(ctx.store.a == 10, "Expected a to be 10 after update")
  assert(ctx.store.b == 10, "Expected b to be 10 after update")
end

return store_update