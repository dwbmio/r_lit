# ---- Build stage (Alpine = native musl toolchain) ----
FROM rust:1.90-alpine AS builder

ARG ALPINE_MIRROR=mirrors.aliyun.com
RUN sed -i "s#://dl-cdn.alpinelinux.org/alpine#://$ALPINE_MIRROR/alpine#g" /etc/apk/repositories \
    && apk add --no-cache cmake make pkgconf musl-dev g++ git ca-certificates curl-dev zlib-dev zlib-static perl linux-headers

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/

# rust-lld (default since 1.86) doesn't search Alpine system lib paths;
# add /usr/lib so it can find -lgcc_s and -lc for proc-macro shared objects.
ENV RUSTFLAGS="-C link-arg=-L/usr/lib"

# Pass CARGO_FEATURES="metric" to enable OTel metrics at build time.
# Alpine native target is already *-unknown-linux-musl, no --target needed.
ARG CARGO_FEATURES=""
ARG CARGO_REGISTRY_MIRROR=sparse+https://rsproxy.cn/index/
RUN mkdir -p /build/.cargo \
    && printf '\n[source.crates-io]\nreplace-with = "china"\n\n[source.china]\nregistry = "%s"\n' "$CARGO_REGISTRY_MIRROR" >> /build/.cargo/config.toml \
    && cargo build --release --features "$CARGO_FEATURES"


# ---- Runtime stage (scratch = zero overhead) ----
FROM scratch

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /build/target/release/pgpour /pgpour
COPY examples/config.example.yml /etc/pgpour/config.yml

ENV RUST_LOG=info
ENV CONFIG_PATH=/etc/pgpour/config.yml

ENTRYPOINT ["/pgpour", "--config", "/etc/pgpour/config.yml"]
