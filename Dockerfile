# The authoritative game server, for Fly.io (fly.toml).
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p kings-cove-server

FROM debian:bookworm-slim
COPY --from=build /src/target/release/kings-cove-server /usr/local/bin/kings-cove-server
ENV KC_ADDR=0.0.0.0:8080 \
    KC_DATA=/data/kings-cove-data.json
EXPOSE 8080
CMD ["kings-cove-server"]
