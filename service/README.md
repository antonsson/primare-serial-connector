# primare-i22-rs232

A lightweight REST API bridge for the **Primare I22** integrated amplifier via RS232, designed for Home Assistant integration. Built with Rust + Axum for minimal resource usage on a Raspberry Pi.

## Features

- Full coverage of the I22 RS232 protocol (power, volume, input, mute, balance, dim, menu, IR input)
- Read back device state (status polling endpoint for HA sensors)
- Verbose mode auto-enabled on startup
- DLE byte escaping/unescaping per spec
- Single binary, ~2MB, ~2MB RAM at idle

## Requirements

- Rust toolchain (`rustup`)
- RS232 to USB adapter (or RS232 HAT for Pi)
- Null modem cable between Pi and I22

## Build

```bash
# Development
cargo build

# Optimised release (recommended for Pi)
cargo build --release

# Cross-compile for Pi from x86 (optional)
cargo install cross
cross build --release --target aarch64-unknown-linux-gnu
```

## Docker Deployment (Raspberry Pi 5)

### 1. Build ARM64 image (on x86 dev machine)

```bash
# Enable buildx for cross-platform builds (one-time)
docker buildx create --use

# Build for ARM64 and export to file
docker buildx build --platform linux/arm64 -t primare-i22-rs232:latest --load .
docker save primare-i22-rs232:latest | gzip > primare-i22-arm64.tar.gz

# Copy to Pi
scp primare-i22-arm64.tar.gz pi@<pi-ip>:~
scp ../docker-compose.yml pi@<pi-ip>:~
```

### 2. Deploy on Raspberry Pi

```bash
# Load the image
gunzip -c primare-i22-arm64.tar.gz | docker load

# Start the service
docker compose up -d

# Verify
curl http://localhost:3000/health
# {"status":"ok","serial_ok":true}
```

### 3. Configure Home Assistant

Copy the plugin to your HA config directory:
```bash
cp -r home-assistant-plugin/* /path/to/ha-config/custom_components/primare_i22/
```

Then in Home Assistant:
1. Restart Home Assistant
2. Go to **Settings → Devices & Services → Add Integration**
3. Search for **Primare I22**
4. Enter the Pi's IP address and port `3000`

The amplifier will appear as a media player entity with power, volume, mute, and source controls.

### Alternative: Build on Pi directly

If building on the Pi itself:
```bash
docker compose up -d --build
```

## Usage

```bash
# Run with defaults (/dev/ttyUSB0, port 3000)
./primare-i22-rs232

# Custom port and listen address
./primare-i22-rs232 --port /dev/ttyUSB0 --listen 0.0.0.0:3000

# All options via environment variables
PRIMARE_PORT=/dev/ttyUSB0 \
PRIMARE_BAUD=4800 \
PRIMARE_LISTEN=0.0.0.0:3000 \
PRIMARE_TIMEOUT_MS=500 \
./primare-i22-rs232
```

## Test Without Physical Amplifier

You can test the real Home Assistant plugin -> real REST service HTTP path by mocking only the serial device.

### 1. Create a virtual serial pair

```bash
# Install once if needed: sudo apt-get install socat
socat -d -d \
  pty,raw,echo=0,link=/tmp/ttyPRIMARE_SERVICE \
  pty,raw,echo=0,link=/tmp/ttyPRIMARE_MOCK
```

Keep this terminal running.

### 2. Run the mock amplifier on one end

```bash
python3 ../tools/mock_amp.py --port /tmp/ttyPRIMARE_MOCK
```

### 3. Run this service on the other end

```bash
PRIMARE_PORT=/tmp/ttyPRIMARE_SERVICE \
PRIMARE_LISTEN=0.0.0.0:3000 \
cargo run
```

### 4. Point Home Assistant integration to this service

Use your machine IP and port `3000` in the Primare I22 integration setup.

## API Reference

### `GET /health`
Health check for container orchestration.
```json
{ "status": "ok", "serial_ok": true }
```

### `GET /status`
Returns all current state in one call (ideal for HA polling).
```json
{
  "power": true,
  "volume": 25,
  "input": 1,
  "mute": false,
  "balance": 0,
  "dim": 2
}
```

### `GET|POST /power`
```json
// POST body
{ "state": "on" }       // or "off" or "toggle"
// Response
{ "power": true }
```

### `GET|POST /volume`
```json
// POST - absolute
{ "level": 25 }         // 0-79
// POST - relative
{ "step": 1 }           // +1 or -1
// Response
{ "volume": 25 }
```

### `GET|POST /input`
```json
// POST - direct
{ "input": 2 }          // 1-7
// POST - step
{ "step": "up" }        // or "down"
// Response
{ "input": 2 }
```

### `GET|POST /mute`
```json
// POST
{ "state": true }       // or false, or omit for toggle
// Response
{ "mute": true }
```

### `GET|POST /balance`
```json
// POST - direct
{ "value": -3 }         // -9 to +9
// POST - step
{ "step": 1 }
// Response
{ "balance": -3 }
```

### `GET|POST /dim`
```json
// POST - direct (0=off, 1-3=brightness)
{ "level": 2 }
// POST - step
{ "step": true }
// Response
{ "dim": 2 }
```

### `POST /menu`
```json
{ "action": "enter" }   // enter, exit, up, down, left, right
```

### `GET|POST /ir_input`
```json
// POST
{ "source": "front" }   // or "back"
// Response
{ "source": "front" }
```

### `GET /info`
```json
{
  "product_line": "PRIMARE",
  "model": "I22",
  "firmware": "V1.1B6"
}
```

### `GET /input/current/name`
```json
{ "name": "CD" }
```

### `GET /input/:id/name`
```json
// GET /input/1/name
{ "name": "CD" }
```

### `POST /factory_reset`
```json
{ "confirm": true }
```

## Home Assistant Integration

### `configuration.yaml` — RESTful sensors

```yaml
rest:
  - resource: http://raspberrypi.local:3000/status
    scan_interval: 5
    sensor:
      - name: "Primare Volume"
        value_template: "{{ value_json.volume }}"
        unit_of_measurement: ""
      - name: "Primare Input"
        value_template: "{{ value_json.input }}"
      - name: "Primare Power"
        value_template: "{{ value_json.power }}"
      - name: "Primare Mute"
        value_template: "{{ value_json.mute }}"
```

### `configuration.yaml` — REST commands

```yaml
rest_command:
  primare_volume_set:
    url: http://raspberrypi.local:3000/volume
    method: POST
    content_type: application/json
    payload: '{"level": {{ level }} }'

  primare_volume_up:
    url: http://raspberrypi.local:3000/volume
    method: POST
    content_type: application/json
    payload: '{"step": 1}'

  primare_volume_down:
    url: http://raspberrypi.local:3000/volume
    method: POST
    content_type: application/json
    payload: '{"step": -1}'

  primare_power_toggle:
    url: http://raspberrypi.local:3000/power
    method: POST
    content_type: application/json
    payload: '{"state": "toggle"}'

  primare_input_set:
    url: http://raspberrypi.local:3000/input
    method: POST
    content_type: application/json
    payload: '{"input": {{ input }} }'

  primare_mute_toggle:
    url: http://raspberrypi.local:3000/mute
    method: POST
    content_type: application/json
    payload: '{}'
```

## systemd Service (Raspberry Pi)

Save as `/etc/systemd/system/primare-i22.service`:

```ini
[Unit]
Description=Primare I22 RS232 REST Bridge
After=network.target

[Service]
Type=simple
User=pi
ExecStart=/usr/local/bin/primare-i22-rs232
Environment=PRIMARE_PORT=/dev/ttyUSB0
Environment=PRIMARE_LISTEN=0.0.0.0:3000
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable --now primare-i22.service
```

## Serial Port Permissions

```bash
sudo usermod -aG dialout pi
# or udev rule:
echo 'SUBSYSTEM=="tty", ATTRS{idVendor}=="XXXX", MODE="0666"' \
  | sudo tee /etc/udev/rules.d/99-primare.rules
```

## Protocol Notes

- Baud: 4800, 8N1, null modem cable
- Byte `0x10` (DLE) must be doubled when it appears as data
- Verbose mode is enabled on startup so all commands return current state
- Volume range: 0–79 (device default is 10)
- Balance range: -9 to +9
- Dim: 0=display off, 1–3=brightness levels
