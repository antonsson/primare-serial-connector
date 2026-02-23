# Primare I22 — Home Assistant Custom Component

A `media_player` integration for the Primare I22 integrated amplifier, talking to the [primare-i22-rs232](https://github.com/YOUR_USERNAME/primare-i22-rs232) REST service running on a Raspberry Pi.

## What you get

- **Power** on/off via HA UI, automations, and voice assistants
- **Volume** slider and step up/down
- **Mute** toggle
- **Source selector** showing your actual input names from the device
- Works with HA dashboards, Alexa, Google Home, and any HA automation

## Installation (Docker / Container)

Your HA config directory (mounted into the container) needs a `custom_components` folder.

### 1. Copy the component

```bash
# Assuming your HA config is at /path/to/ha-config
cp -r custom_components/primare_i22 /path/to/ha-config/custom_components/
```

Your config directory should look like:
```
config/
├── configuration.yaml
├── custom_components/
│   └── primare_i22/
│       ├── __init__.py
│       ├── manifest.json
│       ├── config_flow.py
│       ├── media_player.py
│       ├── const.py
│       └── strings.json
```

### 2. Restart Home Assistant

```bash
docker restart homeassistant
```

### 3. Add the integration

1. Go to **Settings → Devices & Services → Add Integration**
2. Search for **Primare I22**
3. Enter the Raspberry Pi's IP and port (default `3000`)
4. Click **Submit**

HA will connect, fetch the model name and input names from the device, and create the `media_player` entity.

## Network requirements

The HA container needs to reach the Raspberry Pi's REST service. If both are on the same LAN this works out of the box. If HA is in Docker with a bridge network, make sure the Pi's IP is reachable from the container (it usually is via the host network).

## Dashboard card

Add a **Media Control** card pointing to your new entity:

```yaml
type: media-control
entity: media_player.primare_i22
```

Or for a minimal card:

```yaml
type: entities
entities:
  - entity: media_player.primare_i22
```

## Automations example

Turn on the amp when your TV turns on:

```yaml
automation:
  - alias: "Amp on with TV"
    trigger:
      - platform: state
        entity_id: media_player.your_tv
        to: "on"
    action:
      - service: media_player.turn_on
        target:
          entity_id: media_player.primare_i22
      - service: media_player.select_source
        target:
          entity_id: media_player.primare_i22
        data:
          source: "TV"  # must match your input name on the I22
```

## Troubleshooting

**"Cannot connect" during setup** — Check that the REST service is running on the Pi:
```bash
curl http://<pi-ip>:3000/status
```

**Entity unavailable after setup** — Check HA logs (`Settings → System → Logs`) for connection errors. Usually a network or firewall issue.

**Input names show "Input 1" etc.** — The REST service fetches names from the device. Make sure the I22 is powered on during setup. You can rename inputs on the I22 via its menu.
