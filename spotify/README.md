# spotd

Syncs Spotify Connect ([Spotify Soloist](https://developer.spotify.com/documentation/soloist), Spotify's official headless client) with the Primare I22 through the REST service in `../service` (ampd).

```
Spotify app ──Connect──▶ Soloist ──PipeWire──USB──▶ I22
                           │ WebSocket 127.0.0.1:9090
                           ▼
                         spotd ──HTTP──▶ ampd :3000 ──RS232──▶ I22
```

Soloist instead of librespot: librespot-based players can't play on Spotify accounts created after a 2024/2025 cutoff (`audio key error 0 1`, librespot-org/librespot#1649), and Spotify may extend that.

## Behaviour

Pressing play (or selecting the device) in Spotify takes over the amp: power on, switch to the USB input (`-usb-input`).
After that, Spotify "owns" the amp for as long as it stays on **and** on the USB input. Switch to Vinyl or TV and spotd leaves the amp alone (no volume sync, no auto power off) until Spotify plays again.

| Event | Action |
|---|---|
| Spotify device selected / play pressed | Power on if needed, wait for the sound card, switch to USB input, set the Spotify slider to the amp volume |
| Spotify slider moved | Set amp volume (square curve, Spotify 100% = `-max-volume`); only while Spotify owns the amp |
| Amp volume changed (knob/remote) | Move the Spotify slider; only while Spotify owns the amp |
| Amp switched off or to another input while playing | Pause Spotify |
| Spotify paused/stopped (or selected but never played) for `-idle` | Power off, but only if the amp is still on the USB input |

ampd has no event stream, so spotd polls `GET /status` every second, and pauses polling for 2 s after its own writes.
The amp counts as off after two failed polls in a row (ampd errors when the amp is in standby).
Spotify volume events are ignored for 2 s after a takeover, so a new session can't jump the amp to an old level.

Soloist has no fixed-volume mode: it applies the Spotify slider as PipeWire stream volume. A PipeWire stream has two volumes: `channelVolumes` (what Soloist and `wpctl` set and read) and `softVolumes` (the gain actually applied). Soloist reads `channelVolumes` back and reports any change as a slider move, so resetting it with `wpctl` makes spotd fight the remote. spotd leaves `channelVolumes` alone and keeps only `softVolumes` at 1.0 (`pw-cli set-param <id> Props '{ softVolumes: [1.0, 1.0] }'`, on every slider change, at playback start and every 5 s). Once that's set, PipeWire stops copying `channelVolumes` into it, so only the amp sets loudness. Don't use `wpctl set-volume` on the Spotify stream: it would move the Spotify slider and, through spotd, the amp. Check the real gain with `pw-dump <id>` (`softVolumes`).
Soloist runs with `-i 100` so new sessions start at full stream volume.

## Build and test

```bash
go test ./...
CGO_ENABLED=0 go build -trimpath -ldflags=-s -o spotd .
```

## Deploy (Arch, x86_64)

Everything runs as systemd **user** services of one user (Soloist needs PipeWire). ampd stays a system service.

```bash
sudo pacman -S pipewire wireplumber pipewire-pulse
sudo loginctl enable-linger $USER        # start user services at boot without login
systemctl --user enable --now pipewire wireplumber

# Soloist: install from the AUR (provides /usr/bin/soloist), see "Updating Soloist" below
install -Dm600 /dev/null ~/.config/soloist/env
echo 'SOLOIST_API_KEY=<key from developer.spotify.com/dashboard/soloist (Premium)>' > ~/.config/soloist/env
install -Dm644 deploy/soloist.service ~/.config/systemd/user/soloist.service  # -d: PipeWire sink of the I22 (wpctl status)
systemctl --user enable --now soloist
# pick "Vardagsrum" in the Spotify app once to pair

install -Dm755 spotd ~/.local/bin/spotd
install -Dm644 deploy/spotd.service ~/.config/systemd/user/spotd.service   # -usb-input / -card for your wiring
systemctl --user daemon-reload
systemctl --user enable --now spotd
journalctl --user -fu spotd
```

### Updating Soloist

Builds expire 90 days after their build date (`soloist --version` shows it). An expired build exits with code 10 and the unit stops instead of restarting, so Spotify just disappears from the device list. Update:

```bash
yay -Syu                      # or your AUR helper of choice; upgrades /usr/bin/soloist
systemctl --user restart soloist
```

The pairing is kept in `~/.local/share/soloist`.

Find the values:
- `-d` (PipeWire device): `wpctl status` / `pw-cli ls Node` with the amp on; use the I22's `node.name` (`alsa_output.usb-…`).
- `-card`: the I22's name in `/proc/asound/` with the amp on (e.g. `I22`).
- `-usb-input`: `curl localhost:3000/input/<n>/name` for n = 1-7.

Keep the Soloist WebSocket on 127.0.0.1: it has no authentication.

## Flags

| Flag | Default | |
|---|---|---|
| `-usb-input` | (required) | I22 input wired to USB |
| `-max-volume` | 50 | Amp level for Spotify 100% (1-79). Keep ≤ 50 so amp↔Spotify round trips are exact |
| `-idle` | 20m | Power off after Spotify idle this long |
| `-card` | "" | ALSA card id to wait for after power on |
| `-amp` | http://127.0.0.1:3000 | ampd URL |
| `-soloist` | ws://127.0.0.1:9090 | Soloist WebSocket URL (soloist `--ws`) |
| `-stream` | spotify | Soloist's PipeWire stream whose applied gain (softVolumes) is kept at 100% (matches `application.name` or `node.name`, case-insensitive; Soloist sets both to Spotify); empty disables |
| `-poll` | 1s | Amp status poll interval |
