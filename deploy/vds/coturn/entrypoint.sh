#!/bin/sh
set -eu

SECRET_FILE=/svrc/secrets/turn_secret
while [ ! -s "$SECRET_FILE" ]; do
  echo "waiting for the TURN secret from the signal service…"
  sleep 2
done

HOST="${DOMAIN:-${PUBLIC_IP:-}}"
if [ -z "$HOST" ]; then
  echo "Set DOMAIN or PUBLIC_IP" >&2
  exit 1
fi

find_cert() {
  if [ -n "${DOMAIN:-}" ]; then
    dir=$(find /caddy/caddy/certificates -type d -name "$DOMAIN" 2>/dev/null | head -n 1)
    [ -n "$dir" ] && [ -s "$dir/$DOMAIN.crt" ] && [ -s "$dir/$DOMAIN.key" ] || return 1
    CERT="$dir/$DOMAIN.crt"
    KEY="$dir/$DOMAIN.key"
  else
    [ -s /svrc/certs/cert.pem ] && [ -s /svrc/certs/key.pem ] || return 1
    CERT=/svrc/certs/cert.pem
    KEY=/svrc/certs/key.pem
  fi
}

write_config() {
  conf=/tmp/turnserver.conf
  {
    echo "listening-port=${TURN_PORT:-3478}"
    echo "tls-listening-port=${TURNS_PORT:-5349}"
    echo "realm=$HOST"
    echo "server-name=$HOST"
    echo "fingerprint"
    echo "use-auth-secret"
    echo "static-auth-secret=$(cat "$SECRET_FILE")"
    echo "min-port=${TURN_MIN_PORT:-49160}"
    echo "max-port=${TURN_MAX_PORT:-49200}"
    echo "no-cli"
    echo "no-multicast-peers"
    echo "no-tlsv1"
    echo "no-tlsv1_1"
    echo "stale-nonce=600"
    echo "user-quota=12"
    echo "total-quota=600"
    echo "log-file=stdout"
    echo "simple-log"
    echo "proc-user=nobody"
    echo "proc-group=nobody"
    for range in 0.0.0.0-0.255.255.255 10.0.0.0-10.255.255.255 100.64.0.0-100.127.255.255 \
      127.0.0.0-127.255.255.255 169.254.0.0-169.254.255.255 172.16.0.0-172.31.255.255 \
      192.168.0.0-192.168.255.255 ::1 fc00::-fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff \
      fe80::-febf:ffff:ffff:ffff:ffff:ffff:ffff:ffff; do
      echo "denied-peer-ip=$range"
    done
    if [ -n "${TURN_EXTERNAL_IP:-}" ]; then
      echo "external-ip=$TURN_EXTERNAL_IP"
    fi
    if [ -n "${TLS_CERT:-}" ]; then
      echo "cert=$TLS_CERT"
      echo "pkey=$TLS_KEY"
    else
      echo "no-tls"
      echo "no-dtls"
    fi
  } > "$conf"
  chmod 600 "$conf"
}

stamp() {
  if find_cert; then
    cksum "$CERT" | cut -d' ' -f1
  else
    echo none
  fi
}

PID=""
stop() {
  [ -n "$PID" ] && kill "$PID" 2>/dev/null && wait "$PID" 2>/dev/null || true
}
trap 'stop; exit 0' TERM INT

start() {
  TLS_CERT=""
  TLS_KEY=""
  if find_cert; then
    mkdir -p /tmp/certs
    cp "$CERT" /tmp/certs/cert.pem
    cp "$KEY" /tmp/certs/key.pem
    chmod 644 /tmp/certs/cert.pem /tmp/certs/key.pem
    TLS_CERT=/tmp/certs/cert.pem
    TLS_KEY=/tmp/certs/key.pem
  else
    echo "no certificate yet — TURN over TLS stays off until one appears"
  fi
  write_config
  turnserver -c /tmp/turnserver.conf &
  PID=$!
}

CURRENT=$(stamp)
start
while true; do
  sleep 60 &
  wait $! || true
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "turnserver exited — restarting"
    CURRENT=$(stamp)
    start
    continue
  fi
  NEXT=$(stamp)
  if [ "$NEXT" != "$CURRENT" ]; then
    echo "certificate changed — restarting turnserver"
    CURRENT="$NEXT"
    stop
    start
  fi
done
