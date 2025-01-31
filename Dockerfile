FROM rust:1.84.1-alpine AS builder

RUN apk add --no-cache build-base=0.5-r3 git=2.47.2-r0
WORKDIR /usr/src/app
COPY . .
COPY .git .git

ARG TARGETPLATFORM
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=${TARGETPLATFORM} \
    --mount=type=cache,target=/usr/src/app/target,id=${TARGETPLATFORM} \
    sh build.sh

FROM scratch

COPY --from=builder /tmp/lmb /bin/lmb

CMD ["/bin/lmb"]
