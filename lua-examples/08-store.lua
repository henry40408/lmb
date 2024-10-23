local m = require("@lmb")
return m.store:update(function(tx)
	local counter = tx["counter"] or 0
	tx["counter"] = counter + 1
	return tx["counter"]
end)
