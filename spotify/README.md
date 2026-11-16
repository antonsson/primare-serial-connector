# spotd

Syncs Spotify Connect ([go-librespot](https://github.com/devgianlu/go-librespot)) with the Primare I22 through the REST service in `../service` (ampd).

```
Spotify app ──Connect──▶ go-librespot ──ALSA──USB──▶ I22
                           │ REST + WS 127.0.0.1:3678
                           ▼
                         spotd ──HTTP──▶ ampd :3000 ──RS232──▶ I22
```

go-librespot runs with `external_volume: true`, so the stream stays at full level and the amp does all attenuation.

## Behaviour

Pressing play (or selecting the device) in Spotify takes over the amp: power on, switch to the USB input (`-usb-input`).
After that, Spotify "owns" the amp for as long as it stays on **and** on the USB input. Switch to Vinyl or TV and spotd leaves the amp alone (no volume sync, no auto power off) until Spotify plays again.

| Event | Action |
|---|---|
| Spotify device selected / play pressed | Power on if needed, wait for the sound card, switch to USB input, set the Spotify slider to the amp volume |
| Spotify slider moved | Set amp volume (square curve, Spotify 100% = `-max-volume`); only while Spotify owns the amp |
| Amp volume changed (knob/remote) | Move the Spotify slider; only while Spotify owns the amp |
| Amp switched off or to another input while playing | Pause Spotify |
| Spotify paused/stopped for `-idle` | Power off, but only if the amp is still on the USB input |

ampd has no event stream, so spotd polls `GET /status` every second, and pauses polling for 2 s after its own writes.
The amp counts as off after two failed polls in a row (ampd errors when the amp is in standby).
Spotify volume events are ignored for 2 s after a takeover, so a new session can't jump the amp to an old level.

## Build and test

```bash
go test ./...
CGO_ENABLED=0 go build -trimpath -ldflags=-s -o spotd .
```

## Deploy (Arch, x86_64)

```bash
curl -fsSL https://github.com/devgianlu/go-librespot/releases/download/v0.10.6/go-librespot_linux_x86_64.tar.gz \
  | sudo tar -xz -C /usr/local/bin go-librespot
sudo install -m755 spotd /usr/local/bin/
sudo install -Dm644 deploy/config.yml /var/lib/go-librespot/config.yml   # set audio_device
sudo install -m644 deploy/go-librespot.service deploy/spotd.service /etc/systemd/system/  # set CHANGEME values
sudo systemctl daemon-reload
sudo systemctl enable --now go-librespot spotd
journalctl -fu spotd
```

Find the values:
- `audio_device` / `-card`: `aplay -l` with the amp on; the card id is the name in `/proc/asound/` (e.g. `CARD=I22` and `-card I22`).
- `-usb-input`: `curl localhost:3000/input/<n>/name` for n = 1-7.

## Flags

| Flag | Default | |
|---|---|---|
| `-usb-input` | (required) | I22 input wired to USB |
| `-max-volume` | 50 | Amp level for Spotify 100% (1-79). Keep ≤ 50 so amp↔Spotify round trips are exact |
| `-idle` | 20m | Power off after Spotify idle this long |
| `-card` | "" | ALSA card id to wait for after power on |
| `-amp` | http://127.0.0.1:3000 | ampd URL |
| `-librespot` | http://127.0.0.1:3678 | go-librespot API URL |
| `-poll` | 1s | Amp status poll interval |
