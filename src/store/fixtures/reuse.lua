local store = require("@lmb").store
local a = store.a
store.a = a + 1
return a
