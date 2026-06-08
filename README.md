# Shocking VRC

Bridge gap with DG-Lab Coyote and VRChat via OSC.

Terminal UI (ratatui):

```sh
cargo build --release -p shocking_vrc_cli
cargo run --release -p shocking_vrc_cli
```

Optional flags: `--port <udp>` (default 9001), `--scan-timeout <secs>` (default 8).
