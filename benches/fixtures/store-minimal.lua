function f(ctx)
  ctx.store:set("a", true)
  assert(ctx.store:get("a") == true, "ctx.store:get('a') should be true")
end
return f
