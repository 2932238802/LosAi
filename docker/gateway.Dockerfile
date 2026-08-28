FROM rust:1.88-bookworm AS builder

WORKDIR /app

ENV CARGO_NET_OFFLINE=false
ENV CARGO_TERM_COLOR=always

# 复制 workspace 根配置，Cargo 才能解析 apps/api
COPY Cargo.toml Cargo.lock ./
COPY apps/api ./apps/api
COPY migrations ./migrations

RUN cargo build --locked --release -p lostoken-api

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/lostoken-api /usr/local/bin/lostoken-api

EXPOSE 8080

CMD ["/usr/local/bin/lostoken-api"]
