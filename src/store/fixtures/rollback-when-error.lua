local store = require("@lmb").store
return store:update({ "a" }, function(values)
  local a = table.unpack(values)
  assert(a ~= 1, "expect a not to equal 1")
  return table.pack(a + 1)
end, { 0 })
