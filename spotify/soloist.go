package main

import (
	"context"
	"encoding/json"
	"errors"
	"log"
	"sync"
	"time"

	"github.com/coder/websocket"
	"github.com/coder/websocket/wsjson"
)

// event is a Soloist event reduced to what the controller needs.
type event struct {
	kind   string // active, inactive, buffering, playing, paused, idle, volume
	volume int    // 0-100, for kind "volume"
}

// soloistMsg is a Soloist WebSocket message; fields are flat per message type.
// https://developer.spotify.com/documentation/soloist/reference/websocket-api
type soloistMsg struct {
	Type     string `json:"type"`
	Status   string `json:"status"`    // playback_changed, playback_state
	Volume   *int   `json:"volume"`    // volume_changed
	IsActive *bool  `json:"is_active"` // auth_state, device_changed, playback_state
	Message  string `json:"message"`   // error
}

// soloist talks to Spotify Soloist's WebSocket API (started with --ws ADDR:PORT).
// Events and commands share one connection.
type soloist struct {
	url  string
	mu   sync.Mutex
	conn *websocket.Conn
}

// events streams translated events into out, reconnecting until ctx is done.
func (s *soloist) events(ctx context.Context, out chan<- event) {
	for ctx.Err() == nil {
		if err := s.readEvents(ctx, out); err != nil && ctx.Err() == nil {
			log.Printf("soloist: %v (reconnecting)", err)
		}
		select {
		case <-ctx.Done():
		case <-time.After(2 * time.Second):
		}
	}
}

func (s *soloist) readEvents(ctx context.Context, out chan<- event) error {
	conn, _, err := websocket.Dial(ctx, s.url, nil)
	if err != nil {
		return err
	}
	defer conn.CloseNow()
	conn.SetReadLimit(1 << 20) // playback_state and queue_changed carry full entities
	s.mu.Lock()
	s.conn = conn
	s.mu.Unlock()
	defer func() {
		s.mu.Lock()
		s.conn = nil
		s.mu.Unlock()
	}()
	log.Printf("soloist: connected to %s", s.url)
	for {
		_, data, err := conn.Read(ctx)
		if err != nil {
			return err
		}
		var m soloistMsg
		if err := json.Unmarshal(data, &m); err != nil {
			log.Printf("soloist: bad message: %v", err)
			continue
		}
		for _, ev := range translate(m) {
			out <- ev
		}
	}
}

// translate maps a Soloist message to controller events.
func translate(m soloistMsg) []event {
	var evs []event
	switch m.Type {
	case "auth_state", "device_changed":
		if m.IsActive != nil {
			evs = append(evs, event{kind: map[bool]string{true: "active", false: "inactive"}[*m.IsActive]})
		}
	case "playback_changed", "playback_state":
		// playback_state is a snapshot (on connect); its volume is deliberately not forwarded,
		// so restarting spotd never moves the amp.
		if m.Status != "" {
			evs = append(evs, event{kind: m.Status})
		}
	case "volume_changed":
		if m.Volume != nil {
			evs = append(evs, event{kind: "volume", volume: *m.Volume})
		}
	case "error":
		log.Printf("soloist: error: %s", m.Message)
	}
	return evs
}

func (s *soloist) command(ctx context.Context, cmd map[string]any) error {
	s.mu.Lock()
	conn := s.conn
	s.mu.Unlock()
	if conn == nil {
		return errors.New("not connected")
	}
	cmd["type"] = "command"
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	return wsjson.Write(ctx, conn, cmd)
}

func (s *soloist) setVolume(ctx context.Context, v int) error {
	return s.command(ctx, map[string]any{"command": "set_volume", "volume": v})
}

func (s *soloist) pause(ctx context.Context) error {
	return s.command(ctx, map[string]any{"command": "pause"})
}
