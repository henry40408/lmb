function f(ctx)
    -- Store is nil without a connection
    return ctx.store == nil
end
return f
