function f(ctx)
    -- Update with keys that don't exist, using defaults
    local result = ctx.store:update({ x = 100, y = 200 }, function(values)
        values.x = values.x + 1
        values.y = values.y + 2
        return values.x + values.y
    end)
    assert(result == 303, "Expected 303, got " .. tostring(result))
    assert(ctx.store.x == 101, "Expected x=101, got " .. tostring(ctx.store.x))
    assert(ctx.store.y == 202, "Expected y=202, got " .. tostring(ctx.store.y))
end
return f
