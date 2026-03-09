function f(ctx)
    ctx.store:set("你好", "世界")
    ctx.store:set("🔑", { emoji = "🎉" })
    ctx.store:set("キー", 42)

    assert(ctx.store:get("你好") == "世界", "Chinese key/value failed")
    assert(ctx.store:get("🔑").emoji == "🎉", "Emoji key failed")
    assert(ctx.store:get("キー") == 42, "Japanese key failed")
end
return f
