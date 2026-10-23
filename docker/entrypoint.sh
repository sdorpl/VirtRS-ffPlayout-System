#!/bin/sh
# First start: create database, admin user and paths under /data, then run the server.
set -eu

mkdir -p /data/db /data/storage /data/playlists /data/logs /data/public

if [ ! -f "$DB" ]; then
    if [ -z "${ADMIN_PASSWORD:-}" ]; then
        echo "ADMIN_PASSWORD is required on the first start" >&2
        exit 1
    fi

    ffplayout --db "$DB" --init \
        -u "$ADMIN_USER" -p "$ADMIN_PASSWORD" -m "$ADMIN_MAIL" \
        --storage /data/storage --playlists /data/playlists \
        --logs /data/logs --public /data/public \
        --smtp-server "" --smtp-user "" --smtp-password "" \
        --smtp-starttls false --smtp-port 465 --two-factor false
fi

exec ffplayout --db "$DB" --listen "$LISTEN" "$@"
