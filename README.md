# primare-serial-connector

Network control for a **Primare I22** integrated amplifier over its RS232 port, plus Spotify Connect integration that drives the amp automatically.

```
Spotify app ──Connect──▶ Soloist ──PipeWire──USB──▶ I22
                           │ WebSocket
                           ▼
                         spotd ──HTTP──▶ ampd :3000 ──RS232──▶ I22
                                           ▲
                               web UI / curl / scripts
```

## Components

| Directory | What | Language |
|---|---|---|
| [`service/`](service/README.md) | **ampd**: REST bridge to the amp over RS232, with a built-in web UI at `/` | Rust |
| [`spotify/`](spotify/README.md) | **spotd**: syncs Spotify Soloist (official headless Connect client) with ampd: power on, input switch, volume sync, auto power off | Go |
| `tools/mock_amp.py` | Fake I22 on a serial port, for testing without hardware | Python |

ampd's HTTP API (documented in [service/README.md](service/README.md#api-reference)) is the contract between the components.

## Hardware

- Primare I22 with RS232 (4800 baud, 8N1) connected via a null modem cable and a USB-RS232 adapter.
- For Spotify: the I22's USB DAC input connected to the same host.

## Quick start

```bash
# ampd
cd service && cargo run --release -- --port /dev/ttyUSB0 --listen 0.0.0.0:3000
curl localhost:3000/status

# spotd (needs Soloist running; see spotify/README.md)
cd spotify && go build -o spotd . && ./spotd -usb-input 7
```

Deployment (systemd for ampd, systemd user services for Soloist and spotd) is described in each component's README.

## Test

```bash
cd service && cargo test
cd spotify && go test ./...
```

Without an amplifier: run ampd against `tools/mock_amp.py` on a socat pty pair, see [service/README.md](service/README.md#test-without-physical-amplifier).

## Protocol notes

- The amp in standby only answers power commands, and power state can't be read. ampd infers it from traffic and from standby frames the amp pushes.
- Frame format, DLE escaping and variable list: `service/src/protocol.rs`.

## License

MIT, see [LICENSE](LICENSE).
