function store_get_set(ctx)
  ctx.store:set("a", true)
  assert(ctx.store:get("a") == true, "ctx.store:get('a') should be true")

  ctx.store:set("b", 42)
  assert(ctx.store:get("b") == 42, "ctx.store:get('b') should be 42")

  -- get with default
  assert(ctx.store:get("missing") == nil, "missing key should return nil")
  assert(ctx.store:get("missing", { default = 99 }) == 99, "missing key with default should return 99")

  -- has
  assert(ctx.store:has("a") == true, "has('a') should be true")
  assert(ctx.store:has("missing") == false, "has('missing') should be false")
end
return store_get_set
