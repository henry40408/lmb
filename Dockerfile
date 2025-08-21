FROM rust:1.89.0-alpine AS builder

WORKDIR /usr/src/app
RUN apk add --no-cache build-base git
COPY . .
COPY .git .git

RUN cargo build --release

FROM alpine:3.19.1

RUN apk add --no-cache tini=0.19.0-r2
COPY --from=builder /usr/src/app/target/release/lmb /bin/lmb

ENTRYPOINT ["tini", "--"]
CMD ["/bin/lmb"]