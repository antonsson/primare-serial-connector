# CLAUDE.md

Solo repo: commit directly on main, no feature branches.

## Components
- service/  Rust REST bridge to the I22 over RS232 (ampd). Protocol notes in src/protocol.rs.
- spotify/  Go: syncs Spotify Soloist (official headless Connect client) with ampd. See spotify/README.md.

## Test
- cd service && cargo test
- cd spotify && go test ./...
- No hardware: socat pty pair + tools/mock_amp.py (service/README.md)

## Gotchas
- Amp in standby only answers power commands. Power can't be read (W 0x01 0x00 toggles, R gets no reply), so ampd infers it: any non-power frame = on, timeout = standby, plus standby frames the amp pushes (verbose on) on remote/front-panel use.
- A background task reads the port continuously; replies and pushes update a state cache (serial.rs AmpCache).
- W <var> 0x00 is not a pure read: on dim 0x0A it steps brightness (seen on hardware), on mute 0x09 / standby 0x01 the spec calls it a toggle. On step vars (volume, input) it's a 0-step that returns the value.
- /status does live reads for volume/input; power comes from the cache.
- ampd's HTTP API is the contract for spotify/ and the web UI; change it carefully.
