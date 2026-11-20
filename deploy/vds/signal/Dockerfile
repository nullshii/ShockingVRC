FROM rust:1-bookworm AS build
WORKDIR /vds
COPY signal/Cargo.toml signal/Cargo.lock ./signal/
COPY signal/src ./signal/src
COPY web ./web
RUN cd signal && cargo build --release --locked \
    && mkdir -p /out/data \
    && cp target/release/signal /out/signal

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /out/signal /usr/local/bin/signal
COPY --from=build --chown=65532:65532 /out/data /data
USER nonroot
EXPOSE 8080 8081
ENTRYPOINT ["/usr/local/bin/signal"]
CMD ["serve"]
