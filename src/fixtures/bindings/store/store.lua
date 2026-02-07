function store_get(ctx)
  ctx.store.a = true
  assert(ctx.store.a == true, "ctx.store.a should be true")
end

return store_get
