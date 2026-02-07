function serve(ctx)
  return {
    body = "<h1>Hello, World!</h1>",
    headers = { ["content-type"] = "text/html" },
    status_code = 201,
  }
end

return serve
