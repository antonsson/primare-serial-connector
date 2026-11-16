package main

import (
	"context"
	"encoding/json"
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/coder/websocket"
)

// lsEvent is a go-librespot WebSocket event: {"type": "...", "data": {...}}.
type lsEvent struct {
	Type string          `json:"type"`
	Data json.RawMessage `json:"data"`
}

type lsVolume struct {
	Value int `json:"value"`
	Max   int `json:"max"`
}

// librespot talks to the go-librespot API server (server.enabled in its config.yml).
type librespot struct {
	base string
	http *http.Client
}

// events streams WebSocket events into out, reconnecting until ctx is done.
func (l *librespot) events(ctx context.Context, out chan<- lsEvent) {
	url := "ws" + strings.TrimPrefix(l.base, "http") + "/events"
	for ctx.Err() == nil {
		if err := l.readEvents(ctx, url, out); err != nil && ctx.Err() == nil {
			log.Printf("librespot events: %v (reconnecting)", err)
		}
		select {
		case <-ctx.Done():
		case <-time.After(2 * time.Second):
		}
	}
}

func (l *librespot) readEvents(ctx context.Context, url string, out chan<- lsEvent) error {
	conn, _, err := websocket.Dial(ctx, url, nil)
	if err != nil {
		return err
	}
	defer conn.CloseNow()
	conn.SetReadLimit(1 << 20) // metadata events carry long cover URLs
	log.Printf("librespot events: connected to %s", url)
	for {
		_, data, err := conn.Read(ctx)
		if err != nil {
			return err
		}
		var ev lsEvent
		if err := json.Unmarshal(data, &ev); err != nil {
			log.Printf("librespot events: bad message: %v", err)
			continue
		}
		out <- ev
	}
}

func (l *librespot) setVolume(ctx context.Context, v int) error {
	return postJSON(ctx, l.http, l.base+"/player/volume", map[string]int{"volume": v})
}

func (l *librespot) pause(ctx context.Context) error {
	return postJSON(ctx, l.http, l.base+"/player/pause", nil)
}
