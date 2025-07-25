function outside()
  local a = 0
  function inside()
    a = a + 1
    return a
  end
  return inside
end

return outside()
