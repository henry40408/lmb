function list_test(ctx)
  local fs = require("@lmb/fs")
  local entries = fs:list(ctx.state)
  assert(#entries == 2, "Expected 2 entries, got " .. #entries)
  table.sort(entries)
  assert(entries[1] == "a.txt", "Expected first entry to be 'a.txt', got " .. entries[1])
  assert(entries[2] == "b.txt", "Expected second entry to be 'b.txt', got " .. entries[2])
end

return list_test
