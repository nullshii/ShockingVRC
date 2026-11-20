FROM caddy:2-builder AS build
RUN xcaddy build --with github.com/mholt/caddy-l4

FROM caddy:2-alpine
COPY --from=build /usr/bin/caddy /usr/bin/caddy
COPY Caddyfile.domain Caddyfile.ip entrypoint.sh /etc/caddy/
RUN chmod +x /etc/caddy/entrypoint.sh
ENTRYPOINT ["/etc/caddy/entrypoint.sh"]
