FROM rust:1.88.0-bookworm AS builder

ARG BINARY
WORKDIR /workspace
COPY . .
RUN cargo build --locked --release --bin "${BINARY}" \
    && cp "target/release/${BINARY}" /service

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /service /usr/local/bin/service
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/service"]
