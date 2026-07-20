# Shocking VRC

Bridge gap with DG-Lab Coyote and VRChat via OSC.

Terminal UI (ratatui):

```sh
cargo build --release -p shocking_vrc_cli
cargo run --release -p shocking_vrc_cli
```

Optional flags: `--port <udp>` (legacy fixed OSC port; default is automatic via
OSCQuery), `--scan-timeout <secs>` (default 8).
