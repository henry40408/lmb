function test_parse_path()
  local http = require("@lmb/http")

  -- Single parameter extraction
  local params = http.parse_path("/users/42", "/users/{id}")
  assert(params.id == "42", "Expected id '42', got " .. tostring(params.id))

  -- Multiple parameter extraction
  params = http.parse_path("/users/42/posts/99", "/users/{user_id}/posts/{post_id}")
  assert(params.user_id == "42", "Expected user_id '42', got " .. tostring(params.user_id))
  assert(params.post_id == "99", "Expected post_id '99', got " .. tostring(params.post_id))

  -- Exact static path match (no parameters)
  params = http.parse_path("/health", "/health")
  assert(params ~= nil, "Expected table for exact match, got nil")

  -- Non-matching path returns nil
  params = http.parse_path("/other/path", "/users/{id}")
  assert(params == nil, "Expected nil for non-matching path")

  -- Different segment count returns nil
  params = http.parse_path("/users/42/extra", "/users/{id}")
  assert(params == nil, "Expected nil for different segment count")

  -- Empty path
  params = http.parse_path("", "/users/{id}")
  assert(params == nil, "Expected nil for empty path")

  -- Root path
  params = http.parse_path("/", "/")
  assert(params ~= nil, "Expected table for root path match, got nil")

  -- Parameter value with special characters (hyphens)
  params = http.parse_path("/items/my-cool-item", "/items/{slug}")
  assert(params.slug == "my-cool-item", "Expected slug 'my-cool-item', got " .. tostring(params.slug))

  -- Catch-all parameter
  params = http.parse_path("/files/docs/readme.md", "/files/{*rest}")
  assert(params.rest == "docs/readme.md", "Expected rest 'docs/readme.md', got " .. tostring(params.rest))

  return "ok"
end

return test_parse_path
