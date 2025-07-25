function f()
  local a = 1
  error("An error occurred")
  a = a + 1
  return a
end

return f
