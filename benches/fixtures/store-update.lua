function store_tx(ctx)
  ctx.store:tx(function(tx)
    local a = tx:get("a") or 0
    local b = tx:get("b") or 0
    tx:set("a", a - 1)
    tx:set("b", b + 1)
  end)
  assert(ctx.store:get("a") + ctx.store:get("b") == 0, "Expected a + b to be 0 after tx")
end

return store_tx
