function json_path()
  local m = require('@lmb/json-path')
  local nodes = m.query('$.foo.bar', { foo = { bar = 'baz' } })
  assert(nodes[1] == 'baz', 'Expected bar to be "baz", got ' .. nodes[1])
end

return json_path