# lmb

> Lua Function Runner

## Features

- Single binary: Easy to deploy
- Evaluate a function to get a result: In-place computation or one-shot script
- Handle HTTP requests with a script: Long-running HTTP server

## Installation

Install with `go install`:

```sh
go install github.com/henry40408/lmb@latest
lmb --version
```

## Usage

```sh
$ echo 'print("hello, world")' >> hello.lua
$ lmb eval --file hello.lua
hello, world
```

## License

MIT
