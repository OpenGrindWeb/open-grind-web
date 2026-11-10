# syntax=docker/dockerfile:1
# Build context: repository root.
#   docker build -f server/Dockerfile -t og-server .

FROM oven/bun:1 AS front
WORKDIR /src
COPY package.json bun.lock bunfig.toml ./
COPY patches ./patches
COPY scripts ./scripts
RUN bun install --frozen-lockfile
COPY . .
RUN bunx vite build

FROM rust:1.95-bookworm AS server
# boring2 (BoringSSL, pulled in by grindr -> wreq) needs cmake and clang
RUN apt-get update && apt-get install -y --no-install-recommends cmake clang libclang-dev \
	&& rm -rf /var/lib/apt/lists/*
WORKDIR /src/server
COPY server/Cargo.toml server/Cargo.lock* ./
COPY server/src ./src
RUN cargo build --release --locked || cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
	&& rm -rf /var/lib/apt/lists/* \
	&& useradd --system --home /data og
COPY --from=server /src/server/target/release/og-server /usr/local/bin/og-server
COPY --from=front /src/build /srv/og/build
ENV OG_BIND=0.0.0.0:8080 OG_STATIC_DIR=/srv/og/build OG_DATA_DIR=/data
VOLUME /data
USER og
EXPOSE 8080
ENTRYPOINT ["og-server"]
