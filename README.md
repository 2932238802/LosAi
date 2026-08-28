# LosToken

AI API Gateway MVP scaffold based on the Rust Technical Spec. The data plane is Rust/Axum; PostgreSQL stores durable state and Redis is reserved for distributed limits and concurrency.

## Current Phase

Phase 1 is implemented: workspace layout, Axum health service, Vue/Vite control-plane shell, PostgreSQL migration foundation, Redis/PostgreSQL Compose services, environment template, and container build definitions.

## Start

1. Copy .env.example to .env and replace all secrets before non-local use.
2. Run docker compose up -d --build.
3. Gateway health: http://localhost:8080/health
4. Dashboard: http://localhost:5173

## Local checks

Run cargo fmt --check, cargo clippy --workspace --all-targets, cargo test --workspace, cargo build --workspace, and npm run build from apps/web.

## Decisions

Virtual keys will be generated with CSPRNG and stored as a one-way digest. Upstream credentials will remain server-side and encrypted at rest. Credits use integer fixed-point units. The first provider adapter will be OpenAI-compatible; streaming will use reqwest bytes streams and Axum bodies.
