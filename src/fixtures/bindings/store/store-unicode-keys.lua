function f(ctx)
    -- Test Unicode keys
    ctx.store["你好"] = "世界"
    ctx.store["🔑"] = { emoji = "🎉" }
    ctx.store["キー"] = 42

    assert(ctx.store["你好"] == "世界", "Chinese key/value failed")
    assert(ctx.store["🔑"].emoji == "🎉", "Emoji key failed")
    assert(ctx.store["キー"] == 42, "Japanese key failed")
end
return f
