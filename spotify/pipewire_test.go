package main

import (
	"slices"
	"testing"
)

func TestFindStream(t *testing.T) {
	dump := []byte(`[
	 {"id": 48, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "soloist"}}},
	 {"id": 54, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Stream/Output/Audio", "application.name": "Spotify", "media.name": "Spotify", "node.name": "spotify"},
	   "params": {"Props": [{"volume": 1.0, "channelVolumes": [0.064, 0.064], "softVolumes": [0.5, 0.5]}, {"params": []}]}}},
	 {"id": 60, "type": "PipeWire:Interface:Port", "info": {"props": {}}}
	]`)
	s, err := findStream(dump, "spotify")
	if err != nil || s.id != 54 || !slices.Equal(s.soft, []float64{0.5, 0.5}) {
		t.Errorf("findStream(spotify) = %+v, %v; want id 54, soft [0.5 0.5]", s, err)
	}
	if s, _ := findStream(dump, "soloist"); s.id != 0 {
		t.Errorf("findStream(soloist) = %d; want 0 (sink node.name must not match)", s.id)
	}
}
