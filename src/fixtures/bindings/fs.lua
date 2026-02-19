-- Use temp directory for test files
local prefix = "/tmp/lmb_test_fs_"

-- Test write_file and read_file
function test_read_write_file()
  local fs = require("@lmb/fs")
  local path = prefix .. "rw.txt"
  local content = "hello world\n"

  local bytes = fs:write_file(path, content)
  assert(bytes == #content, "Expected " .. #content .. " bytes written, got " .. tostring(bytes))

  local read = fs:read_file(path)
  assert(read == content, "Expected '" .. content .. "' but got '" .. tostring(read) .. "'")

  fs:remove(path)
end

-- Test exists
function test_exists()
  local fs = require("@lmb/fs")
  local path = prefix .. "exists.txt"

  assert(not fs:exists(path), "File should not exist yet")

  fs:write_file(path, "test")
  assert(fs:exists(path), "File should exist after write")

  fs:remove(path)
  assert(not fs:exists(path), "File should not exist after remove")
end

-- Test stat
function test_stat()
  local fs = require("@lmb/fs")
  local path = prefix .. "stat.txt"
  fs:write_file(path, "12345")

  local info = fs:stat(path)
  assert(info.size == 5, "Expected size 5, got " .. tostring(info.size))
  assert(info.is_file == true, "Expected is_file to be true")
  assert(info.is_dir == false, "Expected is_dir to be false")

  fs:remove(path)
end

-- Test open/read/close (low-level)
function test_open_read()
  local fs = require("@lmb/fs")
  local path = prefix .. "open_read.txt"
  fs:write_file(path, "line one\nline two\nline three")

  -- Read all
  local f = fs:open(path, "r")
  local all = f:read("*a")
  assert(all == "line one\nline two\nline three", "read *a failed: " .. tostring(all))
  f:close()

  -- Read line by line
  f = fs:open(path, "r")
  local line1 = f:read("*l")
  assert(line1 == "line one", "Expected 'line one', got '" .. tostring(line1) .. "'")
  local line2 = f:read("*l")
  assert(line2 == "line two", "Expected 'line two', got '" .. tostring(line2) .. "'")
  local line3 = f:read("*l")
  assert(line3 == "line three", "Expected 'line three', got '" .. tostring(line3) .. "'")
  local eof = f:read("*l")
  assert(eof == nil, "Expected nil at EOF, got " .. tostring(eof))
  f:close()

  -- Read N bytes
  f = fs:open(path, "r")
  local chunk = f:read(4)
  assert(chunk == "line", "Expected 'line', got '" .. tostring(chunk) .. "'")
  f:close()

  fs:remove(path)
end

-- Test open/write/close (low-level)
function test_open_write()
  local fs = require("@lmb/fs")
  local path = prefix .. "open_write.txt"

  local f = fs:open(path, "w")
  f:write("hello ")
  f:write("world")
  f:close()

  local content = fs:read_file(path)
  assert(content == "hello world", "Expected 'hello world', got '" .. tostring(content) .. "'")

  fs:remove(path)
end

-- Test append mode
function test_append()
  local fs = require("@lmb/fs")
  local path = prefix .. "append.txt"
  fs:write_file(path, "first")

  local f = fs:open(path, "a")
  f:write(" second")
  f:close()

  local content = fs:read_file(path)
  assert(content == "first second", "Expected 'first second', got '" .. tostring(content) .. "'")

  fs:remove(path)
end

-- Test mkdir and readdir
function test_mkdir_readdir()
  local fs = require("@lmb/fs")
  local dir = prefix .. "dir"

  -- Clean up if exists from previous run
  pcall(function() fs:remove(dir .. "/a.txt") end)
  pcall(function() fs:remove(dir .. "/b.txt") end)

  pcall(function() fs:mkdir(dir) end)

  fs:write_file(dir .. "/a.txt", "a")
  fs:write_file(dir .. "/b.txt", "b")

  local entries = fs:readdir(dir)
  assert(#entries >= 2, "Expected at least 2 entries, got " .. #entries)

  local found_a = false
  local found_b = false
  for _, name in ipairs(entries) do
    if name == "a.txt" then found_a = true end
    if name == "b.txt" then found_b = true end
  end
  assert(found_a, "Expected to find a.txt in directory listing")
  assert(found_b, "Expected to find b.txt in directory listing")

  fs:remove(dir .. "/a.txt")
  fs:remove(dir .. "/b.txt")
end

-- Test error on closed handle
function test_closed_handle()
  local fs = require("@lmb/fs")
  local path = prefix .. "closed.txt"
  fs:write_file(path, "test")

  local f = fs:open(path, "r")
  f:close()

  local ok, err = pcall(function() f:read("*a") end)
  assert(not ok, "Expected error on read after close")
  assert(string.find(tostring(err), "closed"), "Expected 'closed' in error message")

  fs:remove(path)
end

-- Test error: read from write handle
function test_read_from_write_handle()
  local fs = require("@lmb/fs")
  local path = prefix .. "read_write_err.txt"
  local f = fs:open(path, "w")

  local ok, err = pcall(function() f:read("*a") end)
  assert(not ok, "Expected error on read from write handle")
  assert(string.find(tostring(err), "writing"), "Expected 'writing' in error message")

  f:close()
  fs:remove(path)
end

-- Test error: write to read handle
function test_write_to_read_handle()
  local fs = require("@lmb/fs")
  local path = prefix .. "write_read_err.txt"
  fs:write_file(path, "test")
  local f = fs:open(path, "r")

  local ok, err = pcall(function() f:write("data") end)
  assert(not ok, "Expected error on write to read handle")
  assert(string.find(tostring(err), "reading"), "Expected 'reading' in error message")

  f:close()
  fs:remove(path)
end

function test_fs()
  test_read_write_file()
  test_exists()
  test_stat()
  test_open_read()
  test_open_write()
  test_append()
  test_mkdir_readdir()
  test_closed_handle()
  test_read_from_write_handle()
  test_write_to_read_handle()
end

return test_fs
