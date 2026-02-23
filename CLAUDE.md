# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This repository contains a REST API bridge for the Primare I22 integrated amplifier via RS232, plus a Home Assistant custom component that uses it.

Two main components:
- **service/**: Rust REST service that communicates with the I22 over RS232
- **home-assistant-plugin/**: Python Home Assistant `media_player` integration that talks to the REST service

## Build & Run Commands

### Rust Service (service/)

```bash
# Development build
cd service && cargo build

# Release build (optimized for Raspberry Pi)
cd service && cargo build --release

# Run tests
cd service && cargo test

# Cross-compile for Pi
cargo install cross
cd service && cross build --release --target aarch64-unknown-linux-gnu

# Run locally (requires RS232 hardware)
./target/debug/primare-i22-rs232 --port /dev/ttyUSB0 --listen 0.0.0.0:3000
```

Environment variables: `PRIMARE_PORT`, `PRIMARE_BAUD`, `PRIMARE_LISTEN`, `PRIMARE_TIMEOUT_MS`

### Docker Deployment

```bash
# Build and run (from repo root)
docker compose up -d

# Build ARM64 image for Pi 5 from x86
docker buildx build --platform linux/arm64 -t primare-i22-rs232:latest service/

# View logs
docker compose logs -f primare-i22
```

### Home Assistant Plugin

No build step. Copy `home-assistant-plugin/` contents to HA's `custom_components/primare_i22/` directory.

## Architecture

### Rust Service

```
service/src/
├── main.rs      # CLI args, server setup, enables verbose mode on startup
├── api.rs       # Axum routes (/status, /power, /volume, /input, etc.)
├── serial.rs    # SerialConnection: send_recv(), high-level commands (get/set_*)
├── protocol.rs  # RS232 frame building/parsing, DLE byte escaping
├── state.rs     # AppState: Mutex<SerialConnection>
└── error.rs     # AppError enum, ApiResult type
```

Key patterns:
- All serial access goes through `Mutex<SerialConnection>` in `AppState`
- Protocol uses STX/DLE/ETX framing with DLE byte escaping (0x10 doubled when appearing as data)
- "Direct mode" variables use `var | 0x80` to read/set values directly vs relative changes
- Verbose mode is enabled on startup so all commands return state replies

### Protocol Details

- Baud: 4800, 8N1
- Frame format: `<STX> <CMD> <VAR> [<VALUE>] <DLE> <ETX>`
- CMD: 0x57 = Write ('W'), 0x52 = Read ('R')
- Variable IDs defined in `protocol::var` module
- Volume range: 0-79, Balance: -9 to +9, Inputs: 1-7

### Home Assistant Plugin

Python async integration using `aiohttp` to poll the REST service. Implements `MediaPlayerEntity` with power, volume, mute, and source selection features. Polls `/status` at configurable intervals.
