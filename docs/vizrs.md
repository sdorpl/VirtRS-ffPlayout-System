# VizRS Graphics

ffplayout can drive a [VizRS](https://github.com/sdorpl/VizRS-CG-System) graphics
server: when a clip starts, the channel sends VizRS commands over its TCP port
(default `6100`). The same server can be operated by hand in VizRS Trio; its
`/output` page carries the graphics.

Enable it in **Settings → General → VizRS graphics**:

| Field | Meaning |
|---|---|
| Address | `host:port` of the VizRS TCP port, e.g. `127.0.0.1:6100` |
| Token | VizRS `--token`; sent as `AUTH <token>` first. Empty when VizRS runs without a token. |
| Clip | commands sent when a regular clip starts |
| Ad | commands sent when a commercial (`"category": "advertisement"`) starts |
| Burn-in | VizRS `/output` page burned into the program video, e.g. `http://127.0.0.1:8088/output`; empty = off |

One command per line, using the VizRS TCP syntax (`TAKE`, `UPDATE`, `CONTINUE`,
`OUT`, `CLEAR`, `MACRO`, `TICKER`, ...). Empty lines and lines starting with `#`
are ignored.

Placeholders:

| Placeholder | Value |
|---|---|
| `{title}` | clip title, or the file name without extension |
| `{source}` | clip path or URL |
| `{duration}` | played length, `hh:mm:ss` |
| `{next_title}` | title of the next clip |
| `{next_start}` / `{next_start_full}` | start of the next clip, `hh:mm` / `hh:mm:ss` |
| `{next_duration}` | length of the next clip, `hh:mm:ss` |

`|` and line breaks in values are replaced, so they cannot break the
`key=value|key=value` syntax.

Example – "now / next" strap and logo off during ads:

```
TAKE 4001 headline={title}|sub=Next: {next_title} at {next_start}
```

```
OUT main
```

## Commands per clip

A playlist item can carry its own commands in a `vizrs` list. They are sent
after the channel commands:

```json
{
    "in": 0,
    "out": 1800,
    "duration": 1800,
    "source": "/media/news.mp4",
    "title": "News",
    "vizrs": ["TAKE 2001 place=Warsaw|live=LIVE"]
}
```

## Burn-in

With **Burn-in** set, the channel starts a headless Chromium (no GPU) on that
page when playout starts, with the browser viewport set to the channel size
(for example 1280x720). VizRS keeps its 1920x1080 stage and scales it to the
viewport, so do not add `?w=&h=` to the address. Other `/output` parameters
such as `?layers=main,front` work as usual.

Chromium sends a PNG frame with alpha (DevTools screencast) only when the page
changes. ffplayout converts it once, crops it to the visible area and blends it
over every video frame after logo and text, also over fillers and generated
black. Static graphics therefore cost almost nothing; animations cost one PNG
decode per changed frame.

The browser is searched in `PATH` (`chromium`, `chromium-browser`,
`google-chrome`, `google-chrome-stable`, `chrome`, `headless_shell`). Set
`VIZRS_BROWSER` to use another binary. In containers running as root set
`VIZRS_BROWSER_NO_SANDBOX=1`, because Chromium refuses its sandbox there.

If the browser crashes or VizRS is not reachable, the graphics are cleared and
the browser is restarted after 3 seconds; playout continues.
Changing the setting requires a channel restart.

## Failure handling

Graphics never stop the playout. Commands are sent in the background, with a
2 second limit for connecting and for each reply. Errors and `ERR` replies are
written to the channel log; successful replies are logged at debug level.

## Docker (amd64 and arm64)

The root `Dockerfile` builds a server image without GUI and GPU, with
Chromium for burn-in. The `Docker` workflow builds it natively for
`linux/amd64` and `linux/arm64` (GitHub runner `ubuntu-24.04-arm`) and pushes
the multi-arch image `ghcr.io/<owner>/vizrs-ffplayout` with the tags `main`,
`sha-<commit>` and the version for `v*` tags.

```sh
docker run -d --name playout -p 8787:8787 --shm-size 512m \
    -e ADMIN_PASSWORD=change-me -v playout-data:/data \
    ghcr.io/sdorpl/vizrs-ffplayout:main
```

On the first start the entrypoint creates the database and the admin user
(`ADMIN_USER`, default `admin`; `ADMIN_PASSWORD` is required) and the paths
`/data/storage`, `/data/playlists`, `/data/logs`, `/data/public`. Media go to
`/data/storage`. The server runs as user `10001`.

The root `docker-compose.yml` runs VizRS and the playout from the published
images and needs no settings: `docker compose up -d`, or in Portainer paste it
as a stack. On the first start channel 1 is connected to VizRS (`vizrs:6100`,
token, burn-in of `/output`). Change the default passwords and tokens
(`ADMIN_PASSWORD`, `VIZRS_TOKEN`, `VIZRS_OUTPUT_TOKEN`) before opening the ports;
they apply on the first start only.

The entrypoint presets the VizRS settings of channel 1 from `VIZRS_ADDRESS`,
`VIZRS_TOKEN`, `VIZRS_BURN_URL`, `VIZRS_ON_CLIP` and `VIZRS_ON_AD` when it
creates the database. Later changes are made in the panel.

Test emission out of the box: `docker-compose.yml` sets `VIZRS_ON_CLIP=TAKE 3001`
(the ticker page of the VizRS demo show, loaded when VizRS starts with empty
data) and `PLAYOUT_AUTOSTART=1`, which starts channel 1 once through the API on
the first start. The channel stays active after restarts. Without a playlist the
channel plays filler, so the ticker is taken again at every filler clip (60 s)
and its crawl restarts; set `VIZRS_ON_CLIP` empty or `PLAYOUT_AUTOSTART=0`
before the first start to turn this off.

`docker/vizrs/compose.yml` builds VizRS from a local checkout instead.

`docker/vizrs/compose.yml` runs VizRS and the playout together, for example on
an Oracle Cloud Free Ampere A1 instance (arm64):

```sh
cd docker/vizrs
cp env.example .env      # passwords, tokens, path to the VizRS checkout
docker compose up -d
```

Then set in **Settings → General → VizRS graphics**: Address `vizrs:6100`,
Token = `VIZRS_TOKEN`, Burn-in `http://vizrs:8088/output?token=<VIZRS_OUTPUT_TOKEN>`.
Chromium needs a larger `/dev/shm` than the Docker default, hence
`shm_size: 512mb`. Keep encoding CPU-friendly on 4 Ampere cores: 720p25 or
1080p25 with `libx264` and a fast preset.

CLI commands inside the container need `env -u LISTEN`, otherwise they try
to start a second server, e.g. generating a playlist:

```sh
docker exec playout env -u LISTEN ffplayout --db /data/db/ffplayout.db -c 1 -g 2026-10-24
```

The container clock is UTC; playlist dates follow the channel timezone
(Settings → Channel). The HLS stream is at `/public/<channel>/live/stream.m3u8`.

The image is private while the repository is private: log in first with
`docker login ghcr.io` (a token with `read:packages`).

## Panel look and EPG

The panel uses the VizRS themes `vizrs-dark` (default) and `vizrs-light`
(`frontend/src/assets/css/vizrs.css`): the colours, compact panels and top bar
of VizRS Artist/Trio, with the channel clock in the top bar.

The **EPG** page lists the guide for the next 1–14 days and downloads it as
XMLTV; the same document is served by `GET /api/epg/{id}?days=N` (see
[api.md](api.md)). This is the base for a full XMLTV EPG: descriptions,
categories and an unauthenticated feed URL for IPTV clients are not there yet.
