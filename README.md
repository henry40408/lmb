# lmb

> lmb is a Lua function runner

[![Casual Maintenance Intended](https://casuallymaintained.tech/badge.svg)](https://casuallymaintained.tech/)
![GitHub Workflow Status (with event)](https://img.shields.io/github/actions/workflow/status/henry40408/lmb/.github%2Fworkflows%2Fworkflow.yaml)
![GitHub](https://img.shields.io/github/license/henry40408/lmb)
[![codecov](https://codecov.io/gh/henry40408/lmb/graph/badge.svg?token=O7WLYVEX0E)](https://codecov.io/gh/henry40408/lmb)

## Table of Contents

- [Features](#features)
- [Installation](#installation)
- [Usage](#usage)
- Guides
  - [Lua](guides/lua.md)
  - [Serve](guides/serve.md)
- [License](#license)

## Features

- Evaluate Lua scripts.
- Handle HTTP requests via Lua scripts.
- Schedule Lua scripts with cron.

## Installation

### Prerequisites

- Rust ≥ 1.85.0

```bash
git clone https://github.com/henry40408/lmb
cd lmb
cargo install --path . --locked
```

## Usage

Find some examples:

```bash
lmb example ls
```

Evaluate an example:

```bash
lmb example eval --name hello
```

Evaluate a Lua script:

```bash
$ lmb eval --file lua-examples/hello.lua
hello, world!
```

Handle HTTP requests with a single script:

```bash
$ lmb example serve --bind 127.0.0.1:3000 --name echo
(another shell session) $ curl -X POST http://localhost:3000 -d $'hello'
hello
```

## License

MIT
