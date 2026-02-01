# Stage 1: Chef - prepare recipe (runs on build platform)
FROM --platform=$BUILDPLATFORM rust:1.92-alpine AS chef
RUN apk add --no-cache musl-dev g++
RUN cargo install cargo-chef
WORKDIR /app

# Stage 2: Planner - create recipe.json
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Builder - cross-compile for target platform
FROM chef AS builder

ARG TARGETPLATFORM
ARG GIT_VERSION=dev

# Install cross-compilation toolchain based on target
RUN case "$TARGETPLATFORM" in \
        "linux/arm64") \
            apk add --no-cache aarch64-none-elf-gcc && \
            rustup target add aarch64-unknown-linux-musl \
            ;; \
        "linux/amd64") \
            rustup target add x86_64-unknown-linux-musl \
            ;; \
    esac

WORKDIR /app

# Configure cargo for cross-compilation
RUN mkdir -p .cargo && \
    case "$TARGETPLATFORM" in \
        "linux/arm64") \
            echo '[target.aarch64-unknown-linux-musl]' >> .cargo/config.toml && \
            echo 'linker = "aarch64-none-elf-gcc"' >> .cargo/config.toml \
            ;; \
    esac

# Set the Rust target based on platform
RUN case "$TARGETPLATFORM" in \
        "linux/arm64") echo "aarch64-unknown-linux-musl" > /tmp/rust_target ;; \
        "linux/amd64") echo "x86_64-unknown-linux-musl" > /tmp/rust_target ;; \
        *) echo "x86_64-unknown-linux-musl" > /tmp/rust_target ;; \
    esac

COPY --from=planner /app/recipe.json recipe.json

# Cook dependencies with target
RUN RUST_TARGET=$(cat /tmp/rust_target) && \
    cargo chef cook --release --recipe-path recipe.json --target $RUST_TARGET

COPY . .

# Build the application
RUN RUST_TARGET=$(cat /tmp/rust_target) && \
    GIT_VERSION=${GIT_VERSION} cargo build --release --target $RUST_TARGET && \
    cp target/$RUST_TARGET/release/lmb /app/lmb

# Stage 4: Runtime - distroless for minimal image
FROM gcr.io/distroless/static-debian12

COPY --from=builder /app/lmb /lmb

ENTRYPOINT ["/lmb"]
