FROM rust:1.86.0 AS builder

RUN apt-get update
RUN apt-get install -qqy build-essential git

WORKDIR /usr/src/app
COPY . .
COPY .git .git

ARG TARGETPLATFORM
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=${TARGETPLATFORM} \
    --mount=type=cache,target=/usr/src/app/target,id=${TARGETPLATFORM} \
    bash build.sh

FROM gcr.io/distroless/cc-debian12

COPY --from=builder /tmp/lmb /bin/lmb

CMD ["/bin/lmb"]
