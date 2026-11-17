package main

import "testing"

func TestFindStream(t *testing.T) {
	dump := []byte(`[
	 {"id": 48, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "soloist"}}},
	 {"id": 54, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Stream/Output/Audio", "application.name": "Spotify", "media.name": "Spotify", "node.name": "spotify"}}},
	 {"id": 60, "type": "PipeWire:Interface:Port", "info": {"props": {}}}
	]`)
	for _, m := range []string{"spotify"} {
		if id, err := findStream(dump, m); err != nil || id != 54 {
			t.Errorf("findStream(%q) = %d, %v; want 54", m, id, err)
		}
	}
	if id, _ := findStream(dump, "soloist"); id != 0 {
		t.Errorf("findStream(soloist) = %d; want 0 (sink node.name must not match)", id)
	}
}

func TestParseVolume(t *testing.T) {
	for in, want := range map[string]float64{"Volume: 0.40\n": 0.40, "Volume: 1.00 [MUTED]\n": 1.0} {
		if v, err := parseVolume(in); err != nil || v != want {
			t.Errorf("parseVolume(%q) = %v, %v", in, v, err)
		}
	}
	if _, err := parseVolume("garbage"); err == nil {
		t.Error("want error")
	}
}
