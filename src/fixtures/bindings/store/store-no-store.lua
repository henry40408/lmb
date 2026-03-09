function f(ctx)
    -- get returns nil
    assert(ctx.store:get("k") == nil, "get should return nil")

    -- set returns nil (no-op)
    assert(ctx.store:set("k", 1) == nil, "set should return nil")

    -- del returns false
    assert(ctx.store:del("k") == false, "del should return false")

    -- has returns false
    assert(ctx.store:has("k") == false, "has should return false")

    -- keys returns empty table
    local k = ctx.store:keys()
    assert(#k == 0, "keys should return empty table")

    -- tx raises error
    local ok, err = pcall(function()
        ctx.store:tx(function(tx) end)
    end)
    assert(not ok, "tx should raise error")
end
return f
