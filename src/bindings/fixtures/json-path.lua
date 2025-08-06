function json_path()
  local m = require('@lmb/json-path')

  local nodes = m.query('$.foo.bar', { foo = { bar = 'baz' } })
  assert(#nodes == 1, 'Expected 1 node, got ' .. #nodes)
  assert(nodes[1] == 'baz', 'Expected bar to be "baz", got ' .. nodes[1])

  local data = {
    { value = 1, name = 'one' },
    { value = 2, name = 'two' },
    { value = 3, name = 'three' }
  }
  local nodes = m.query('$[*].name', data)
  assert(#nodes == 3, 'Expected 3 names, got ' .. #nodes)
  assert(nodes[1] == 'one', 'Expected first name to be "one", got ' .. nodes[1])
  assert(nodes[2] == 'two', 'Expected second name to be "two", got ' .. nodes[2])
  assert(nodes[3] == 'three', 'Expected third name to be "three", got ' .. nodes[3])
end

return json_path
