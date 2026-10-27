# Stage 1: Build binary
FROM rust:1.80-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations

# Compile production release binary
RUN cargo build --release -p ledger-server

# Stage 2: Minimal runtime image
FROM alpine:3.20

RUN apk add --no-cache ca-certificates tzdata
WORKDIR /app

COPY --from=builder /app/target/release/ledger-server /usr/local/bin/ledger-server
COPY migrations ./migrations

EXPOSE 8080

ENTRYPOINT ["ledger-server"]
