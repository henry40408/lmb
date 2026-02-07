function f(ctx)
    -- Set initial values
    ctx.store.preserved = "original"
    ctx.store.modified = 0

    -- Only modify 'modified', leave 'preserved' alone in update
    ctx.store:update({ modified = 0 }, function(values)
        values.modified = values.modified + 10
    end)

    -- 'preserved' should still be the original value
    assert(ctx.store.preserved == "original", "preserved should be untouched")
    assert(ctx.store.modified == 10, "modified should be 10")
end
return f
