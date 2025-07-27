# lmb

> A Lua function runner

## What's lmb?

It's a simple command-line tool that allows you to run Lua scripts and functions directly from the terminal. Its performance is optimized for quick execution of Lua code snippets so it's suitable for low-end hardware such as Raspberry Pi.

```bash
$ cat > hello.lua <<EOF
> function hello()
>     print("Hello, world!")
> end
> return hello
> EOF

$ lmb eval --file hello.lua
Hello, world!
```

## Features

- Run Lua scripts and functions from the command line.

## Installation

You can install `lmb` using `cargo`:

```bash
$ cargo install --git https://github.com/henry40408/lmb.git
```

## License

MIT
