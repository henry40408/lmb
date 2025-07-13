local store = require("@lmb").store
return store:update({ "a" }, function(s)
  s.a = s.a + 1
end, { a = 0 })
