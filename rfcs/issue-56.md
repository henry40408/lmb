# Manage functions with REST API

* Issue: [#56](https://github.com/henry40408/lmb/issues/56)
* Author: [@henry40408](https://github.com/henry40408)

## How to start admin process to manage functions and normal process to handle requests

For example, to start admin process on port 3000 and handle requests on port 3001:

```sh
./target/release/lmb serve --admin-bind 127.0.0.1:3000 --bind 127.0.0.1:3001
```

## Database schema

```sql
CREATE TABLE functions (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    script TEXT NOT NULL,
    method TEXT NOT NULL, -- HTTP method to handle requests e.g. GET, POST, etc.
    path TEXT NOT NULL, -- path to handle requests e.g. /foo/bar
    -- order of the function to handle requests e.g. 1, 2, 3, ...
    -- the smaller the order, the earlier the function is executed
    order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
) STRICT;
```

## APIs

### Handle requests

* `ANY /`: handle requests with Lua function and root path.
* `ANY /{*path}`: handle requests with Lua function and path.

#### How to find functions

1. Find functions with the method and path, ordered by `order` and `updated_at`:

```sql
-- find functions with the method and path, ordered by order and updated_at
SELECT * FROM functions WHERE method = ? AND path = ? ORDER BY order ASC, updated_at DESC;
```

2. `next()` is a function to call the next function in the middleware chain.
3. If no function is found, return `404 Not Found`.

## Admin APIs

### List functions

`GET /v1/functions`

Response body:

```json
[
    {
        "name": "foo",
        "script": "return true",
        "method": "GET",
        "path": "/foo/bar",
        "order": 1,
        "created_at": "2024-01-01T00:00:00Z",
        "updated_at": "2024-01-01T00:00:00Z"
    }
]
```

### Get function

`GET /v1/functions/:name`

Response body:

```json
{
    "name": "foo",
    "script": "return true",
    "method": "GET",
    "path": "/foo/bar",
    "order": 1,
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-01T00:00:00Z"
}
```

### Create function

`POST /v1/functions`

Request body:

```json
{
    "name": "foo",
    "script": "return true",
    "method": "GET",
    "path": "/foo/bar",
    "order": 1
}
```

The previous request body will create a function handling requests with `GET /foo/bar`.

### Update function

`PUT /v1/functions/:name`

Request body:

```json
{
    "script": "return true",
    "method": "GET",
    "path": "/foo/bar",
    "order": 1
}
```

### Delete function

`DELETE /v1/functions/:name`
