# syntax=docker/dockerfile:1.3-labs
FROM rust:1.89.0-bookworm AS chef 
RUN cargo install cargo-chef --locked
WORKDIR app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

ARG TARGETARCH
RUN <<EOF
set -ex
case "${TARGETARCH}" in
  amd64) target='x86_64-unknown-linux-gnu';;
  arm64) target='aarch64-unknown-linux-gnu';;
  *) echo "Unsupported architecture: ${TARGETARCH}" && exit 1;;
esac
cargo chef cook --release --target "${target}" --recipe-path recipe.json
EOF

COPY . .
COPY .git .git

RUN <<EOF
set -ex
case "${TARGETARCH}" in
  amd64) target='x86_64-unknown-linux-gnu';;
  arm64) target='aarch64-unknown-linux-gnu';;
  *) echo "Unsupported architecture: ${TARGETARCH}" && exit 1;;
esac
cargo build --release --target "${target}"
mv /app/target/${target}/release/lmb /bin/lmb
EOF

FROM gcr.io/distroless/cc-debian12 AS runtime
COPY --from=builder /bin/lmb /bin/lmb
CMD ["/bin/lmb"]