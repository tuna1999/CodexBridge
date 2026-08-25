ARG RUST_VERSION=1.88

FROM docker.io/library/rust:${RUST_VERSION}-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY examples ./examples
RUN cargo build --locked --release --bin codex-bridge

FROM docker.io/library/rust:${RUST_VERSION}-bookworm

LABEL org.opencontainers.image.title="CodexBridge" \
      org.opencontainers.image.description="Production Codex-style MCP coding-agent bridge"

# Keep a practical coding environment in the runtime image. Podman is present
# as an execution primitive; project AGENTS.md decides how a project uses it.
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        bubblewrap \
        ca-certificates \
        curl \
        git \
        jq \
        podman \
        podman-compose \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /build/target/release/codex-bridge /usr/local/bin/codex-bridge

RUN mkdir -p /workspace

# With rootless Podman, container UID 0 maps to the unprivileged host account
# that owns podman.socket. That mapping lets the client open the socket without
# making it world-writable. Do not deploy this image with a rootful socket.
USER 0:0
ENV MCP_BIND=0.0.0.0:3000 \
    RUST_LOG=info

EXPOSE 3000
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:3000/health >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/codex-bridge"]
