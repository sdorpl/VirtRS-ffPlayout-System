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

# Test card for the first start (docker/entrypoint.sh): 60 s FFmpeg testsrc2 + 1 kHz tone, 720p25.
FROM debian:trixie-slim AS card

RUN apt-get update && \
    apt-get --assume-yes install --no-install-recommends ffmpeg && \
    ffmpeg -v error -f lavfi -i testsrc2=size=1280x720:rate=25:duration=60 \
        -f lavfi -i sine=frequency=1000:sample_rate=48000:duration=60 \
        -c:v libx264 -preset veryfast -crf 28 -pix_fmt yuv420p -g 50 \
        -c:a aac -b:a 96k -shortest -movflags +faststart /plansza.mp4

FROM debian:trixie-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get --assume-yes install --no-install-recommends \
        ca-certificates \
        chromium \
        curl \
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
COPY --from=card /plansza.mp4 /usr/share/ffplayout/plansza.mp4
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
