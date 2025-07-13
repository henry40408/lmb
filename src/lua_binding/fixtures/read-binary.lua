local s = io.read("*a")
local t = {}
for b in (s or ""):gmatch(".") do
  table.insert(t, string.byte(b))
end
return t
