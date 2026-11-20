#!/bin/sh
set -eu

if [ -n "${DOMAIN:-}" ]; then
  if [ -z "${EMAIL:-}" ]; then
    echo "EMAIL is required with DOMAIN (Let's Encrypt account contact)" >&2
    exit 1
  fi
  exec caddy run --config /etc/caddy/Caddyfile.domain --adapter caddyfile
fi

if [ -z "${PUBLIC_IP:-}" ]; then
  echo "Set DOMAIN (and EMAIL), or PUBLIC_IP for an IP-only server" >&2
  exit 1
fi
i=0
while [ ! -s /svrc/certs/cert.pem ] || [ ! -s /svrc/certs/key.pem ]; do
  i=$((i + 1))
  if [ "$i" -gt 60 ]; then
    echo "the signal service did not create its certificate" >&2
    exit 1
  fi
  echo "waiting for the self-signed certificate…"
  sleep 2
done
exec caddy run --config /etc/caddy/Caddyfile.ip --adapter caddyfile
