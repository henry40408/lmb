function f(ctx)
    ctx.store:set("user:1", "alice")
    ctx.store:set("user:2", "bob")
    ctx.store:set("item:1", "sword")

    -- All keys
    local all = ctx.store:keys()
    assert(#all == 3, "Expected 3 keys, got " .. #all)

    -- Pattern match
    local users = ctx.store:keys("user:%")
    assert(#users == 2, "Expected 2 user keys, got " .. #users)

    local items = ctx.store:keys("item:%")
    assert(#items == 1, "Expected 1 item key, got " .. #items)
end
return f
