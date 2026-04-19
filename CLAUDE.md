# Suzuri (硯) — GPU-Accelerated Terminal Emulator

Rust-native terminal emulator using wgpu (Metal/Vulkan) for rendering.
Pleme-io's replacement for Ghostty.

## Build & Test

```bash
cargo build          # compile
cargo clippy         # lint
cargo test           # 13 unit tests
cargo run            # launch terminal
```

## Architecture

### Pipeline

```
Keyboard → input.rs → PTY write
PTY read → vte::Parser → Terminal grid update
Terminal grid → Renderer (wgpu) → Screen
```

### Module Map

| Path | Purpose |
|------|---------|
| `src/main.rs` | winit event loop, lifecycle orchestration |
| `src/config.rs` | TOML config + shikumi hot-reload |
| `src/terminal/mod.rs` | Terminal grid, VTE handler (scrollback, alt screen, SGR) |
| `src/terminal/cell.rs` | Cell, Color, CellAttrs data types |
| `src/pty.rs` | PTY spawning + reader/waiter threads |
| `src/renderer/mod.rs` | wgpu surface, text blit, cell background quads |
| `src/renderer/atlas.rs` | Glyph atlas (cosmic-text font system) |
| `src/input.rs` | Keyboard → PTY byte mapping (xterm sequences) |
| `src/errors.rs` | Error types |
| `assets/bg.wgsl` | Cell background quad shader |
| `assets/blit.wgsl` | Fullscreen text texture blit shader |

### pleme-io Library Reuse

| Library | Usage |
|---------|-------|
| **shikumi** | Config discovery, hot-reload, ArcSwap store |
| **substrate** | Nix build infrastructure (planned) |
| **frost** | Default shell candidate (future) |

### Key Dependencies

| Crate | Purpose |
|-------|---------|
| `vte` | VT100/xterm escape sequence parser |
| `portable-pty` | Cross-platform pseudo-terminal |
| `winit` | Window management |
| `wgpu` | GPU rendering (Metal on macOS) |
| `cosmic-text` | Font shaping and glyph rasterization |
| `shikumi` | Config discovery + hot-reload |
| `bytemuck` | GPU vertex data casting |

### Config

Configuration lives at `~/.config/suzuri/suzuri.toml` with hot-reload via
shikumi. Environment overrides use `SUZURI_` prefix (e.g. `SUZURI_FONT_SIZE=16`).

Default color scheme is Nord.

### Terminal Emulation

The `Terminal` struct implements `vte::Perform` and handles:
- Full SGR (Select Graphic Rendition) including 256-color and true color
- Cursor movement (CUU, CUD, CUF, CUB, CUP)
- Erase operations (ED, EL, DCH, ICH, IL, DL)
- Scroll regions (DECSTBM)
- Alternate screen buffer (DECSET 1049)
- Auto-wrap at line end
- Scrollback buffer

### Rendering Strategy

1. CPU-side text rasterization via cosmic-text → pixel buffer
2. Upload pixel buffer as wgpu texture
3. Fullscreen blit shader draws text
4. Cell background quads drawn on top for colored cells
5. Cursor rendered as a semi-transparent overlay quad

Future: move to per-glyph GPU rendering with texture atlas for better
performance at large grid sizes.

### Testing

Tests are deterministic and platform-independent:
- Terminal grid operations (put_char, linefeed, scroll, erase, resize)
- VTE integration (print, SGR)
- Input mapping (keys → PTY bytes)
- Alt screen save/restore
