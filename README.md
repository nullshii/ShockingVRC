# Shocking VRC

Bridge gap with DG-Lab Coyote and VRChat via OSC.

## Desktop GUI (egui):
```sh
cargo run --release -p shocking_vrc_gui
```
## Terminal UI (ratatui):
```sh
cargo run --release -p shocking_vrc_cli
```

Both accept the same flags: `--port <udp>` (legacy fixed OSC port; the default
is automatic via OSCQuery), `--scan-timeout <secs>` (default 8),
`--skip-update-check`, and `--mode classic|graph` (run this session in a mode
other than the saved one).

## Programming Modes

The app has two programming modes:

- **Classic** (default) — Configure avatar zones, channels, tuning, modulation, and presets.
- **Graph** — Build your own logic by connecting avatar inputs, processing nodes, and a Device node for each Coyote, similar to a shader editor.

Switch modes in **Setup → Preferences → Mode**. Only one mode controls the devices at a time. Your Classic settings and presets remain unchanged when you switch to Graph.

### Graph Features

- **Groups:** Select nodes and press `Ctrl+G` to group them into a single node with input and output ports. Press `Tab` to enter a group.
- **Frames:** Organize nodes visually.
- **Reroutes:** Redirect connections to keep the graph tidy.
- **Presets:** Use the side panel to replace the entire graph (with confirmation and one undo step) or insert a saved section into the current graph. Device references are stored as numbered slots, not MAC addresses.

### Quick Start

1. Go to **Setup → Preferences → Mode**, select **Graph**, and confirm.
2. Connect a Coyote. Its Device node will appear on the canvas.
3. Right-click the canvas or press `Shift+A`, then select **Contact (raw)**. Choose a zone in the inspector, or leave it set to `Any / *`.
4. Connect the Contact node's **Level** output to the Device node's **Channel A → Power** input.

### Keyboard Shortcuts

| Action | Shortcut |
|---|---|
| Add a node | Right-click or `Shift+A` |
| Connect nodes | Drag between pins |
| Cut wires | `Ctrl` + drag |
| Insert a node into a wire | Drag the node onto the wire |
| Delete | `Delete` |
| Copy / Paste / Duplicate | `Ctrl+C` / `Ctrl+V` / `Ctrl+D` |
| Undo / Redo | `Ctrl+Z` / `Ctrl+Shift+Z` |
| Add frames | `F` |
| Collapse nodes | `H` |
| Group / Ungroup | `Ctrl+G` / `Ctrl+Shift+G` |
| Pan the canvas | Middle mouse drag or `Space` + drag |
| Zoom | Mouse wheel |
| Edit a value | Double-click it and type |
| Emergency stop | `F12` |