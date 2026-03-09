function f(ctx)
    ctx.store:set("x", 42)
    assert(ctx.store:has("x") == true, "x should exist")

    local deleted = ctx.store:del("x")
    assert(deleted == true, "del should return true for existing key")
    assert(ctx.store:has("x") == false, "x should not exist after del")
    assert(ctx.store:get("x") == nil, "get should return nil after del")

    -- Deleting non-existent key returns false
    local deleted2 = ctx.store:del("x")
    assert(deleted2 == false, "del should return false for non-existent key")
end
return f
