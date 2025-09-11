function f(ctx)
  return ctx.store:update({ a = 0 }, function(values)
    values.a = values.a + 1
    return values.a
  end)
end

return f