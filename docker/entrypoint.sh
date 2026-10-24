#!/bin/sh
# First start: create database, admin user and paths under /data, seed channel 1
# with the test card (playlists + filler), preset its VizRS connection, start it
# and take the start graphics in VizRS, then run the server.
set -eu

mkdir -p /data/db /data/storage /data/playlists /data/logs /data/public

# TOML basic string: escape backslash and double quote.
toml_string() {
    printf '"%s"' "$(printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g')"
}

# Test card built into the image (Dockerfile, docker/plansza.mp4).
CARD=/usr/share/ffplayout/plansza.mp4

# In a container the server sets up shared storage: channel 1 keeps its media
# and playlists in the subfolder 1.
STORAGE=/data/storage/1
PLAYLISTS=/data/playlists/1

# Copies the test card to the storage and fills the playlists from yesterday
# (the playout day starts at day_start) to SEED_DAYS days ahead with it; later
# days play it as filler. One day is generated, the others are copies of it.
seed_channel() {
    [ "${SEED_TEST_CARD:-1}" = 1 ] && [ -f "$CARD" ] || return 0

    mkdir -p "$STORAGE" "$PLAYLISTS"
    cp "$CARD" "$STORAGE/plansza.mp4"
    first=$(date -d '-1 day' +%F)
    env -u LISTEN ffplayout --db "$DB" -c 1 --generate "$first" > /dev/null
    template=$PLAYLISTS/$(date -d "$first" +%Y/%m)/$first.json

    for day in $(seq 0 "${SEED_DAYS:-31}"); do
        date=$(date -d "$first +$day days" +%F)
        dir=$PLAYLISTS/$(date -d "$date" +%Y/%m)
        mkdir -p "$dir"
        [ -f "$dir/$date.json" ] ||
            sed "s/\"date\": *\"$first\"/\"date\": \"$date\"/" "$template" > "$dir/$date.json"
    done

    echo "Test card playlists for channel 1 from $first, ${SEED_DAYS:-31} days ahead"
}

# Rewrites the config of channel 1: test card as filler, [vizrs] table from
# VIZRS_* variables.
preset_channel() {
    work=$(mktemp -d)
    cd "$work"
    env -u LISTEN ffplayout --db "$DB" --dump-config -c 1 > /dev/null
    if [ -n "${VIZRS_ADDRESS:-}" ]; then
        awk '/^\[vizrs\]$/ { skip = 1; next } /^\[/ { skip = 0 } !skip' ffplayout_1.toml > config.toml
        preset_vizrs >> config.toml
        echo "VizRS preset for channel 1: $VIZRS_ADDRESS, burn-in ${VIZRS_BURN_URL:-off}"
    else
        cp ffplayout_1.toml config.toml
    fi

    if [ -f "$STORAGE/plansza.mp4" ]; then
        sed -i 's|^filler = .*|filler = "plansza.mp4"|' config.toml
    fi

    env -u LISTEN ffplayout --db "$DB" --import-config config.toml -c 1 > /dev/null
    cd /
    rm -rf "$work"
}

# [vizrs] table of channel 1 from VIZRS_* variables.
preset_vizrs() {
    {
        echo
        echo "[vizrs]"
        echo "enable = true"
        echo "address = $(toml_string "$VIZRS_ADDRESS")"
        echo "token = $(toml_string "${VIZRS_TOKEN:-}")"
        echo "on_clip = $(toml_string "${VIZRS_ON_CLIP:-}")"
        echo "on_ad = $(toml_string "${VIZRS_ON_AD:-}")"
        echo "burn_url = $(toml_string "${VIZRS_BURN_URL:-}")"
    }
}

# Starts playout of channel 1 once through the API; the server keeps the channel
# active in the database, so it plays again after every restart.
autostart_channel() {
    [ "${PLAYOUT_AUTOSTART:-1}" = 1 ] || return 0

    port=${LISTEN##*:}
    api="http://127.0.0.1:$port"
    login=$(printf '{"username":"%s","password":"%s"}' "$ADMIN_USER" "$ADMIN_PASSWORD")

    for _ in $(seq 1 60); do
        sleep 2
        token=$(curl -fsS -H 'Content-Type: application/json' -d "$login" "$api/auth/login" 2>/dev/null |
            sed -n 's/.*"access":"\([^"]*\)".*/\1/p') || true
        [ -n "$token" ] || continue

        if curl -fsS -H "Authorization: Bearer $token" -H 'Content-Type: application/json' \
            -d '{"command":"start"}' "$api/api/control/1/process" > /dev/null; then
            echo "Playout of channel 1 started"
            take_start_graphics
            return 0
        fi
    done

    echo "Autostart of channel 1 failed, start it in the panel" >&2
}

# Takes VIZRS_START_PAGE once through the VizRS HTTP API, with the
# VIZRS_START_VALUES fields (key=value|key=value). VizRS does not keep it after
# its own restart; take it again in Trio then.
take_start_graphics() {
    [ -n "${VIZRS_HTTP:-}" ] && [ -n "${VIZRS_START_PAGE:-}" ] || return 0

    set -- -G --data-urlencode "token=${VIZRS_TOKEN:-}"
    rest=${VIZRS_START_VALUES:-}
    while [ -n "$rest" ]; do
        field=${rest%%|*}
        [ -z "$field" ] || set -- "$@" --data-urlencode "$field"
        [ "$rest" = "$field" ] && rest= || rest=${rest#*|}
    done

    for _ in $(seq 1 30); do
        if curl -fsS "$@" "$VIZRS_HTTP/api/take/$VIZRS_START_PAGE" > /dev/null 2>&1; then
            echo "VizRS: page $VIZRS_START_PAGE taken"
            return 0
        fi
        sleep 2
    done

    echo "VizRS: page $VIZRS_START_PAGE could not be taken at $VIZRS_HTTP" >&2
}

if [ ! -f "$DB" ]; then
    if [ -z "${ADMIN_PASSWORD:-}" ]; then
        echo "ADMIN_PASSWORD is required on the first start" >&2
        exit 1
    fi

    env -u LISTEN ffplayout --db "$DB" --init \
        -u "$ADMIN_USER" -p "$ADMIN_PASSWORD" -m "$ADMIN_MAIL" \
        --storage /data/storage --playlists /data/playlists \
        --logs /data/logs --public /data/public \
        --smtp-server "" --smtp-user "" --smtp-password "" \
        --smtp-starttls false --smtp-port 465 --two-factor false

    seed_channel
    preset_channel
    autostart_channel &
fi

exec ffplayout --db "$DB" --listen "$LISTEN" "$@"
