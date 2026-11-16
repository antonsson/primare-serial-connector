# CLAUDE.md

Solo repo: commit directly on main, no feature branches.

## Components
- service/  Rust REST bridge to the I22 over RS232 (ampd). Protocol notes in src/protocol.rs.
- spotify/  Go: syncs go-librespot (Spotify Connect) with ampd. See spotify/README.md.
- home-assistant-plugin/  Python HA media_player, polls ampd /status.

## Test
- cd service && cargo test
- cd spotify && go test ./...
- No hardware: socat pty pair + tools/mock_amp.py (service/README.md)

## Gotchas
- Amp in standby doesn't answer: reads time out, ampd returns an error and marks power off.
- /status does live serial reads (no cache); power is cached.
- ampd's HTTP API is the contract for spotify/ and the HA plugin; change it carefully.
