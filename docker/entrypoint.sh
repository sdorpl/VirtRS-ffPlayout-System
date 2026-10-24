#!/bin/sh
# First start: create database, admin user and paths under /data, preset the
# VizRS connection of channel 1, then run the server.
set -eu

mkdir -p /data/db /data/storage /data/playlists /data/logs /data/public

# TOML basic string: escape backslash and double quote.
toml_string() {
    printf '"%s"' "$(printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g')"
}

# Replaces the [vizrs] table of channel 1 with values from VIZRS_* variables.
preset_vizrs() {
    [ -n "${VIZRS_ADDRESS:-}" ] || return 0

    work=$(mktemp -d)
    cd "$work"
    env -u LISTEN ffplayout --db "$DB" --dump-config -c 1 > /dev/null

    awk '/^\[vizrs\]$/ { skip = 1; next } /^\[/ { skip = 0 } !skip' ffplayout_1.toml > config.toml
    {
        echo
        echo "[vizrs]"
        echo "enable = true"
        echo "address = $(toml_string "$VIZRS_ADDRESS")"
        echo "token = $(toml_string "${VIZRS_TOKEN:-}")"
        echo "on_clip = $(toml_string "${VIZRS_ON_CLIP:-}")"
        echo "on_ad = $(toml_string "${VIZRS_ON_AD:-}")"
        echo "burn_url = $(toml_string "${VIZRS_BURN_URL:-}")"
    } >> config.toml

    env -u LISTEN ffplayout --db "$DB" --import-config config.toml -c 1 > /dev/null
    cd /
    rm -rf "$work"
    echo "VizRS preset for channel 1: $VIZRS_ADDRESS, burn-in ${VIZRS_BURN_URL:-off}"
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
            return 0
        fi
    done

    echo "Autostart of channel 1 failed, start it in the panel" >&2
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

    preset_vizrs
    autostart_channel &
fi

exec ffplayout --db "$DB" --listen "$LISTEN" "$@"
