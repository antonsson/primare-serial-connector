package main

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"
)

func TestVolumeRoundTrip(t *testing.T) {
	// Amp knob change -> Spotify slider -> volume event must map back to the same level,
	// otherwise the sync would nudge the amp. Holds while ampMax <= volume steps / 2.
	for _, ampMax := range []int{30, 50} {
		for a := 0; a <= ampMax; a++ {
			if got := toAmp(toSpotify(a, 100, ampMax), 100, ampMax); got != a {
				t.Errorf("ampMax=%d: amp %d -> spotify %d -> amp %d", ampMax, a, toSpotify(a, 100, ampMax), got)
			}
		}
	}
	if got := toAmp(100, 100, 50); got != 50 {
		t.Errorf("Spotify 100%% = amp %d, want cap 50", got)
	}
	if got := toSpotify(79, 100, 50); got != 100 {
		t.Errorf("amp above cap = Spotify %d, want 100", got)
	}
}

// recorder is a fake HTTP server that logs "METHOD /path body" lines.
type recorder struct {
	mu    sync.Mutex
	calls []string
}

func (r *recorder) server(t *testing.T) *httptest.Server {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
		b, _ := io.ReadAll(req.Body)
		r.mu.Lock()
		r.calls = append(r.calls, req.Method+" "+req.URL.Path+" "+string(b))
		r.mu.Unlock()
		json.NewEncoder(w).Encode(ampStatus{Power: true, Volume: 20, Input: 3})
	}))
	t.Cleanup(s.Close)
	return s
}

func (r *recorder) has(prefix string) bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	for _, c := range r.calls {
		if len(c) >= len(prefix) && c[:len(prefix)] == prefix {
			return true
		}
	}
	return false
}

func newTestController(t *testing.T) (*controller, *recorder, *recorder) {
	amp, ls := &recorder{}, &recorder{}
	cfg := config{usbInput: 3, maxVolume: 50, idle: time.Minute}
	c := newController(cfg, &ampClient{amp.server(t).URL, http.DefaultClient}, &librespot{ls.server(t).URL, http.DefaultClient})
	return c, amp, ls
}

var ctx = context.Background()

func TestInputChangedWhilePlayingPausesSpotify(t *testing.T) {
	c, _, ls := newTestController(t)
	c.st, c.playing = ampStatus{Power: true, Volume: 20, Input: 3}, true
	c.onAmp(ctx, polled{st: ampStatus{Power: true, Volume: 20, Input: 1}, started: time.Now()})
	if !ls.has("POST /player/pause") {
		t.Fatal("expected pause when input switched to vinyl")
	}
	if !c.idleSince.IsZero() {
		t.Fatal("idle timer must not run for another source")
	}
}

func TestSinglePollErrorIsIgnored(t *testing.T) {
	c, _, ls := newTestController(t)
	c.st, c.playing = ampStatus{Power: true, Input: 3}, true
	c.onAmp(ctx, polled{err: errors.New("timeout"), started: time.Now()})
	if ls.has("POST /player/pause") || !c.owned(c.st) {
		t.Fatal("one failed poll must not count as amp off")
	}
	c.onAmp(ctx, polled{err: errors.New("timeout"), started: time.Now()})
	if !ls.has("POST /player/pause") {
		t.Fatal("two failed polls means amp off: expected pause")
	}
}

func TestStalePollIsDropped(t *testing.T) {
	c, _, ls := newTestController(t)
	c.st, c.playing = ampStatus{Power: true, Input: 3}, true
	started := time.Now()
	c.lastWrite.Store(time.Now().Add(time.Millisecond).UnixNano())
	c.onAmp(ctx, polled{st: ampStatus{Power: false}, started: started})
	if ls.has("POST /player/pause") {
		t.Fatal("poll started before our write must be ignored")
	}
}

func TestIdlePowerOffOnlyForSpotifySource(t *testing.T) {
	c, amp, _ := newTestController(t)
	c.st, c.idleSince = ampStatus{Power: true, Input: 1}, time.Now().Add(-time.Hour)
	c.checkIdle(ctx)
	if amp.has("POST /power") {
		t.Fatal("must not power off while another input is selected")
	}
	c.st, c.idleSince = ampStatus{Power: true, Input: 3}, time.Now().Add(-time.Hour)
	c.checkIdle(ctx)
	if !amp.has(`POST /power {"state":"off"}`) {
		t.Fatal("expected power off after Spotify idle on USB input")
	}
}

func TestSpotifyVolumeIgnoredOnOtherSource(t *testing.T) {
	c, _, _ := newTestController(t)
	c.st = ampStatus{Power: true, Volume: 20, Input: 1}
	c.onEvent(ctx, lsEvent{Type: "volume", Data: json.RawMessage(`{"value":90,"max":100}`)})
	select {
	case <-c.volSignal:
		t.Fatal("Spotify volume must not reach the amp while TV/Vinyl is selected")
	default:
	}
}

func TestClaimSwitchesInputAndSyncsSlider(t *testing.T) {
	c, amp, ls := newTestController(t)
	c.st = ampStatus{Power: true, Volume: 20, Input: 1}
	c.onEvent(ctx, lsEvent{Type: "playing"})
	if !amp.has(`POST /input {"input":3}`) {
		t.Fatal("expected switch to USB input")
	}
	if amp.has("POST /power") {
		t.Fatal("amp already on: no power command expected")
	}
	if !ls.has(`POST /player/volume {"volume":63}`) { // amp 20 of max 50
		t.Fatal("expected Spotify slider set from amp volume")
	}
}
