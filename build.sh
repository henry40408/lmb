#!/usr/bin/env bash

set -euo pipefail

if [[ "$TARGETPLATFORM" = "linux/amd64" ]]; then
	rustup target add x86_64-unknown-linux-gnu
	cargo build --release --target x86_64-unknown-linux-gnu
	cp target/x86_64-unknown-linux-gnu/release/lmb /tmp/lmb
elif [[ "$TARGETPLATFORM" = "linux/arm64" ]]; then
	rustup target add aarch64-unknown-linux-gnu
	cargo build --release --target aarch64-unknown-linux-gnu
	cp target/aarch64-unknown-linux-gnu/release/lmb /tmp/lmb
else
	echo "target platform $TARGETPLATFORM not supported"
	exit 1
fi