local store = require("@lmb").store
return store:update({ "a" }, function(s)
  s.a = s.a + 1
  store:update({ "b" }, function(t)
    t.b = t.b + 1
  end)
end)
