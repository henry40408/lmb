# Stage 1: Chef - prepare recipe (runs on build platform)
FROM --platform=$BUILDPLATFORM rust:1.92-alpine AS chef

ARG TARGETPLATFORM

# Install base build dependencies
RUN apk add --no-cache musl-dev g++ curl

# Download musl.cc cross-compiler for arm64 cross-compilation
RUN if [ "$TARGETPLATFORM" = "linux/arm64" ]; then \
        curl -fsSL https://musl.cc/aarch64-linux-musl-cross.tgz | tar -xz -C /opt && \
        ln -s /opt/aarch64-linux-musl-cross/bin/* /usr/local/bin/; \
    fi

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

# Add Rust target based on platform
RUN case "$TARGETPLATFORM" in \
        "linux/arm64") rustup target add aarch64-unknown-linux-musl ;; \
        "linux/amd64") rustup target add x86_64-unknown-linux-musl ;; \
    esac

WORKDIR /app

# Configure cargo for cross-compilation
RUN mkdir -p .cargo && \
    case "$TARGETPLATFORM" in \
        "linux/arm64") \
            echo '[target.aarch64-unknown-linux-musl]' >> .cargo/config.toml && \
            echo 'linker = "aarch64-linux-musl-gcc"' >> .cargo/config.toml \
            ;; \
    esac

# Set environment variables for cc crate cross-compilation
ENV CC_aarch64_unknown_linux_musl=aarch64-linux-musl-gcc
ENV CXX_aarch64_unknown_linux_musl=aarch64-linux-musl-g++
ENV AR_aarch64_unknown_linux_musl=aarch64-linux-musl-ar

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
