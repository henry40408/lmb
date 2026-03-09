function f(ctx)
    ctx.store:set("a", 10)
    ctx.store:set("b", 20)

    -- Error in tx should rollback all changes
    local ok, err = pcall(function()
        ctx.store:tx(function(tx)
            tx:set("a", 999)
            tx:set("b", 999)
            error("abort transaction")
        end)
    end)

    assert(not ok, "Expected error from tx")
    assert(ctx.store:get("a") == 10, "Expected a=10 after rollback, got " .. tostring(ctx.store:get("a")))
    assert(ctx.store:get("b") == 20, "Expected b=20 after rollback, got " .. tostring(ctx.store:get("b")))
end
return f
