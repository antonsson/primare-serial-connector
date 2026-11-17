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

// streamPinner keeps the gain PipeWire applies to Soloist's stream at 100%.
// Soloist applies the Spotify slider as stream volume, and spotd also maps the
// slider to the amp, so without this the volume would be applied twice.
//
// A stream has two volumes: channelVolumes (what wpctl and Soloist set and read)
// and softVolumes (the gain actually applied). Soloist reads channelVolumes back
// and reports any change as a Spotify slider move, so resetting it fights the
// remote. spotd only sets softVolumes: Soloist doesn't see that, and once set
// explicitly PipeWire stops copying channelVolumes into it (verified on 1.6).
type streamPinner struct {
	match string // matched against the stream's binary / application / node name
	kick  chan struct{}
}

func newStreamPinner(match string) *streamPinner {
	return &streamPinner{match: strings.ToLower(match), kick: make(chan struct{}, 1)}
}

// poke asks for a check soon; never blocks. Safe on a nil pinner.
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
			p.pin(ctx, true)
		}
	}
}

// pin sets the stream's softVolumes to 1.0 if it exists and isn't there already.
// kicked = called because of a volume/playback event (log a missing stream).
func (p *streamPinner) pin(ctx context.Context, kicked bool) {
	out, err := exec.CommandContext(ctx, "pw-dump").Output()
	if err != nil {
		log.Printf("pipewire: pw-dump: %v", err)
		return
	}
	s, err := findStream(out, p.match)
	if err != nil {
		log.Printf("pipewire: %v", err)
		return
	}
	if s.id == 0 {
		if kicked {
			log.Printf("pipewire: no playback stream matching %q (check: wpctl inspect <id> | grep -E 'process.binary|application.name|node.name')", p.match)
		}
		return
	}
	if len(s.soft) == 0 || allOne(s.soft) {
		return
	}
	ones := strings.TrimSuffix(strings.Repeat("1.0, ", len(s.soft)), ", ")
	if err := exec.CommandContext(ctx, "pw-cli", "set-param", strconv.Itoa(s.id), "Props", "{ softVolumes: ["+ones+"] }").Run(); err != nil {
		log.Printf("pipewire: set softVolumes on %d: %v", s.id, err)
		return
	}
	log.Printf("pipewire: Soloist stream %d softVolumes %v -> 1.0", s.id, s.soft)
}

func allOne(v []float64) bool {
	for _, x := range v {
		if x != 1.0 {
			return false
		}
	}
	return true
}

type stream struct {
	id   int // 0 = not found
	soft []float64
}

// findStream returns the playback stream whose binary, application or node
// name equals match (case-insensitive), with its softVolumes.
func findStream(dump []byte, match string) (stream, error) {
	var objs []struct {
		ID   int    `json:"id"`
		Type string `json:"type"`
		Info struct {
			Props  map[string]any `json:"props"`
			Params struct {
				Props []struct {
					SoftVolumes []float64 `json:"softVolumes"`
				} `json:"Props"`
			} `json:"params"`
		} `json:"info"`
	}
	if err := json.Unmarshal(dump, &objs); err != nil {
		return stream{}, fmt.Errorf("pw-dump: %w", err)
	}
	for _, o := range objs {
		props := o.Info.Props
		if o.Type != "PipeWire:Interface:Node" || props["media.class"] != "Stream/Output/Audio" {
			continue
		}
		for _, k := range []string{"application.process.binary", "application.name", "node.name"} {
			if s, ok := props[k].(string); ok && strings.ToLower(s) == match {
				st := stream{id: o.ID}
				for _, pp := range o.Info.Params.Props {
					if len(pp.SoftVolumes) > 0 {
						st.soft = pp.SoftVolumes
					}
				}
				return st, nil
			}
		}
	}
	return stream{}, nil
}
