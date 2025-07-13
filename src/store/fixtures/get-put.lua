local store = require("@lmb").store
local a = store.a
assert(not store.b, "Expect b not to exist")
store.a = 4.56
return a
