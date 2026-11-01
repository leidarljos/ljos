# Builds ljos-mcp and starts it on standard input and output.
# A registry that lists a server by running it and reading its tools, such as Glama, builds this.
FROM rust:1.89.0-slim-bookworm AS build
# rust-toolchain.toml pins 1.89.0 with clippy and rustfmt; the build needs neither.
ENV RUSTUP_TOOLCHAIN=1.89.0
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p ljos --bin ljos-mcp

FROM debian:bookworm-slim
RUN useradd --create-home --shell /usr/sbin/nologin ljos
COPY --from=build /src/target/release/ljos-mcp /usr/local/bin/ljos-mcp
USER ljos
WORKDIR /home/ljos
CMD ["ljos-mcp"]
