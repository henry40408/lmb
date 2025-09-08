# lmb

[![Casual Maintenance Intended](https://casuallymaintained.tech/badge.svg)](https://casuallymaintained.tech/)
![GitHub Workflow Status (with event)](https://img.shields.io/github/actions/workflow/status/henry40408/lmb/.github%2Fworkflows%2Fworkflow.yaml)
![GitHub](https://img.shields.io/github/license/henry40408/lmb)
[![codecov](https://codecov.io/gh/henry40408/lmb/graph/badge.svg?token=O7WLYVEX0E)](https://codecov.io/gh/henry40408/lmb)

## What is lmb?

A standalone [Luau](https://luau.org) runtime.

It is a simple command-line tool that allows you to run Lua scripts and functions directly from the terminal. Its performance is optimized for quick execution of Lua code snippets, making it suitable for low-end hardware such as the Raspberry Pi.

```bash
$ cat > hello.lua <<EOF
> function hello()
>     print("Hello, world!")
> end
> return hello
EOF

$ lmb eval --file hello.lua
Hello, world!
```

For more information, please read [the guided tour](docs/guided-tour.md).

## Features

- Batteries included: Comes with handy libraries such as `crypto`, `http`, and `json`.
- Easy to use: Run Lua scripts and functions from the command line.
- Fast: Optimized for quick execution, making it suitable for low-end hardware.
- Secure: Runs Lua code in a sandboxed environment provided by Luau to prevent unwanted side effects.

## Installation

You can install `lmb` using `cargo`:

```bash
$ cargo install --git https://github.com/henry40408/lmb.git --locked
```

## Similar projects

- [Lune](https://github.com/lune-org/lune): I discovered this project through an [issue](https://github.com/mlua-rs/mlua/issues/620) in the mlua repository. If you need a Lua runtime with features for the Roblox platform, you might want to try Lune. Since I do not develop for the Roblox platform, I do not plan to add similar features to this project in the near future.

## License

MIT
