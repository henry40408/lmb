local m = require("@lmb")
for k, v in pairs(m:getenvs()) do
  print(k .. " = " .. v)
end
