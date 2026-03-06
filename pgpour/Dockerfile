# ---- Build stage (Alpine = native musl toolchain) ----
FROM rust:1.88-alpine AS builder

RUN apk add --no-cache cmake make pkgconf musl-dev g++ git ca-certificates curl-dev zlib-dev zlib-static perl linux-headers

WORKDIR /build
COPY .cargo/ .cargo/
COPY rust-toolchain.toml Cargo.toml Cargo.lock* ./
COPY src/ src/

# Pass CARGO_FEATURES="metric" to enable OTel metrics at build time.
# Alpine native target is already *-unknown-linux-musl, no --target needed.
# For cross-arch prod builds: docker build --platform linux/amd64
ARG CARGO_FEATURES=""
RUN cargo build --release --features "$CARGO_FEATURES"

# ---- Runtime stage (scratch = zero overhead) ----
FROM scratch

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /build/target/release/pgpour /pgpour
COPY config.example.yml /etc/pgpour/config.yml

ENV RUST_LOG=info
ENV CONFIG_PATH=/etc/pgpour/config.yml

ENTRYPOINT ["/pgpour", "--config", "/etc/pgpour/config.yml"]
