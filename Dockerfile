# VizRS ffPlayout System – server image (no GUI, no GPU), linux/amd64 and linux/arm64.
#
#   docker build -t vizrs-ffplayout .
#   docker run -d -p 8787:8787 -e ADMIN_PASSWORD=change-me -v ffplayout-data:/data vizrs-ffplayout
#
# Built natively per architecture (CI: ubuntu-24.04-arm for arm64), no emulation.
# Chromium is included for VizRS burn-in (docs/vizrs.md).

FROM rust:slim-trixie AS build
WORKDIR /src

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_INCREMENTAL=0

RUN apt-get update && \
    apt-get --assume-yes install --no-install-recommends \
        ca-certificates \
        clang \
        libavcodec-dev \
        libavformat-dev \
        libavutil-dev \
        libasound2-dev \
        libclang-dev \
        libsrt-openssl-dev \
        libswresample-dev \
        libswscale-dev \
        nodejs \
        npm \
        pkg-config && \
    rm -rf /var/lib/apt/lists/*

COPY . .

RUN npm ci && \
    npm run build-only && \
    cargo build --release --locked --package ffplayout --no-default-features --features embed_frontend && \
    cp target/release/ffplayout /ffplayout && \
    /ffplayout --version

FROM debian:trixie-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get --assume-yes install --no-install-recommends \
        ca-certificates \
        chromium \
        fonts-dejavu-core \
        libasound2t64 \
        libavcodec61 \
        libavformat61 \
        libavutil59 \
        libsrt1.5-openssl \
        libswresample5 \
        libswscale8 \
        tini && \
    rm -rf /var/lib/apt/lists/* && \
    useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin ffplayout && \
    mkdir -p /data /usr/share/ffplayout && \
    chown 10001:10001 /data

COPY --from=build /ffplayout /usr/bin/ffplayout
COPY assets/dummy.vtt assets/logo.png /usr/share/ffplayout/
COPY docker/entrypoint.sh /entrypoint.sh

USER 10001:10001

# Chromium inside a container cannot use its own sandbox without extra privileges.
ENV DB=/data/db/ffplayout.db \
    LISTEN=0.0.0.0:8787 \
    VIZRS_BROWSER=/usr/bin/chromium \
    VIZRS_BROWSER_NO_SANDBOX=1 \
    ADMIN_USER=admin \
    ADMIN_MAIL=admin@localhost

VOLUME /data
EXPOSE 8787
ENTRYPOINT ["/usr/bin/tini", "--", "/entrypoint.sh"]
