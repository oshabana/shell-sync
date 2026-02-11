# Multi-stage build for shell-sync
FROM node:22-slim AS web-builder
WORKDIR /app/web-ui
COPY web-ui/package.json web-ui/package-lock.json* ./
RUN npm ci --no-audit
COPY web-ui/ ./
RUN npm run build

FROM rust:1.83-slim AS builder
RUN apt-get update && apt-get install -y pkg-config cmake && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY crates/ crates/
COPY --from=web-builder /app/web-ui/dist web-ui/dist
# Build release binary
RUN cargo build --release --bin shell-sync

# Runtime image
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/shell-sync /usr/local/bin/shell-sync

EXPOSE 8888
VOLUME ["/data", "/git-repo"]

ENV DB_PATH=/data/sync.db
ENV GIT_REPO_PATH=/git-repo

ENTRYPOINT ["shell-sync"]
CMD ["serve", "--foreground"]
