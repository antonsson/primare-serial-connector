package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"os/exec"
	"strconv"
	"strings"
	"time"
)

// streamPinner keeps Soloist's PipeWire stream volume at 100%. Soloist applies
// the Spotify slider as PipeWire stream volume, and spotd also maps the slider to
// the amp, so without this the volume would be applied twice.
type streamPinner struct {
	match string // matched against the stream's binary / application / node name
	kick  chan struct{}
	id    int // cached node id; 0 = unknown
}

func newStreamPinner(match string) *streamPinner {
	return &streamPinner{match: strings.ToLower(match), kick: make(chan struct{}, 1)}
}

// poke asks for a reset soon; never blocks. Safe on a nil pinner.
func (p *streamPinner) poke() {
	if p == nil {
		return
	}
	select {
	case p.kick <- struct{}{}:
	default:
	}
}

func (p *streamPinner) run(ctx context.Context) {
	t := time.NewTicker(5 * time.Second)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
			p.pin(ctx, false)
		case <-p.kick:
			// Soloist may apply the new volume just after its WebSocket event.
			p.pin(ctx, true)
			select {
			case <-ctx.Done():
				return
			case <-time.After(300 * time.Millisecond):
			}
			p.pin(ctx, true)
		}
	}
}

// pin sets the stream to 1.0 if it exists and isn't there already.
// kicked = called because of a volume/playback event (log a missing stream).
func (p *streamPinner) pin(ctx context.Context, kicked bool) {
	if p.id != 0 {
		if v, err := getVolume(ctx, p.id); err == nil {
			if v != 1.0 {
				p.set(ctx, v)
			}
			return
		}
		p.id = 0 // stream gone (Soloist restarted or stopped playing)
	}
	id, err := p.find(ctx)
	if err != nil {
		log.Printf("pipewire: %v", err)
		return
	}
	if id == 0 {
		if kicked {
			log.Printf("pipewire: no playback stream matching %q (check: wpctl inspect <id> | grep -E 'process.binary|application.name|node.name')", p.match)
		}
		return
	}
	log.Printf("pipewire: tracking Soloist stream %d", id)
	p.id = id
	if v, err := getVolume(ctx, id); err == nil && v != 1.0 {
		p.set(ctx, v)
	}
}

func (p *streamPinner) set(ctx context.Context, from float64) {
	if err := exec.CommandContext(ctx, "wpctl", "set-volume", strconv.Itoa(p.id), "1.0").Run(); err != nil {
		log.Printf("pipewire: set-volume %d: %v", p.id, err)
		return
	}
	log.Printf("pipewire: Soloist stream %d volume %.2f -> 1.00", p.id, from)
}

// find returns the id of the playback stream whose binary, application or node
// name equals p.match (case-insensitive), or 0 if there is none.
func (p *streamPinner) find(ctx context.Context) (int, error) {
	out, err := exec.CommandContext(ctx, "pw-dump").Output()
	if err != nil {
		return 0, fmt.Errorf("pw-dump: %w", err)
	}
	return findStream(out, p.match)
}

func findStream(dump []byte, match string) (int, error) {
	var objs []struct {
		ID   int    `json:"id"`
		Type string `json:"type"`
		Info struct {
			Props map[string]any `json:"props"`
		} `json:"info"`
	}
	if err := json.Unmarshal(dump, &objs); err != nil {
		return 0, fmt.Errorf("pw-dump: %w", err)
	}
	for _, o := range objs {
		props := o.Info.Props
		if o.Type != "PipeWire:Interface:Node" || props["media.class"] != "Stream/Output/Audio" {
			continue
		}
		for _, k := range []string{"application.process.binary", "application.name", "node.name"} {
			if s, ok := props[k].(string); ok && strings.ToLower(s) == match {
				return o.ID, nil
			}
		}
	}
	return 0, nil
}

// getVolume parses `wpctl get-volume <id>`: "Volume: 0.40" (optionally " [MUTED]").
func getVolume(ctx context.Context, id int) (float64, error) {
	out, err := exec.CommandContext(ctx, "wpctl", "get-volume", strconv.Itoa(id)).Output()
	if err != nil {
		return 0, err
	}
	return parseVolume(string(out))
}

func parseVolume(s string) (float64, error) {
	f := strings.Fields(s)
	if len(f) < 2 || f[0] != "Volume:" {
		return 0, fmt.Errorf("unexpected wpctl output %q", s)
	}
	return strconv.ParseFloat(f[1], 64)
}
