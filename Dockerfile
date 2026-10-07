# Build environment for pdit: Rust, the WebAssembly target and the Dioxus CLI.
# Used for local development and, later, for building the site in CI.
FROM rust:1.98.1-slim-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl unzip pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32-unknown-unknown \
    && rustup component add clippy rustfmt \
    && cargo install dioxus-cli --version 0.7.10 --locked \
    && rm -rf "$CARGO_HOME/registry"

WORKDIR /work
