function f(ctx)
    ctx.store:set("a", 100)
    ctx.store:set("b", 0)

    -- Atomic transfer: a -> b
    local result = ctx.store:tx(function(tx)
        local a = tx:get("a")
        local b = tx:get("b")
        tx:set("a", a - 30)
        tx:set("b", b + 30)
        return a - 30
    end)

    assert(result == 70, "Expected tx to return 70, got " .. tostring(result))
    assert(ctx.store:get("a") == 70, "Expected a=70, got " .. tostring(ctx.store:get("a")))
    assert(ctx.store:get("b") == 30, "Expected b=30, got " .. tostring(ctx.store:get("b")))

    -- tx can read keys not declared upfront
    ctx.store:set("c", "hello")
    ctx.store:tx(function(tx)
        local c = tx:get("c")
        tx:set("c", c .. " world")
    end)
    assert(ctx.store:get("c") == "hello world", "Expected c='hello world'")

    -- tx:del works
    ctx.store:tx(function(tx)
        tx:del("c")
    end)
    assert(ctx.store:get("c") == nil, "Expected c to be deleted")
end
return f
