# Allergen Label Printer – web server for Home Assistant and browsers.
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY res ./res
COPY examples ./examples
RUN cargo build --release --features server --bin labels-server

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tzdata curl \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/labels-server /usr/local/bin/labels-server
ENV DATA_DIR=/data CERT_DIR=/certs PORT=8080 TZ=Europe/London
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s CMD curl -fsk https://localhost:8080/api/health || curl -fs http://localhost:8080/api/health || exit 1
CMD ["labels-server"]
