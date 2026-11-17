package main

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"
)

func TestVolumeRoundTrip(t *testing.T) {
	// Amp knob change -> Spotify slider -> volume event must map back to the same level,
	// otherwise the sync would nudge the amp. Holds while ampMax <= volume steps.
	for _, ampMax := range []int{30, 50, 79} {
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

func TestTranslate(t *testing.T) {
	cases := map[string][]event{
		`{"type":"device_changed","is_active":true,"device_name":"x"}`: {{kind: "active"}},
		`{"type":"auth_state","logged_in":true,"is_active":false}`:     {{kind: "inactive"}},
		`{"type":"playback_changed","status":"paused"}`:                {{kind: "paused"}},
		`{"type":"playback_state","status":"playing","volume":65}`:     {{kind: "playing"}}, // snapshot volume not forwarded
		`{"type":"volume_changed","volume":42}`:                        {{kind: "volume", volume: 42}},
		`{"type":"position_sync","position":{}}`:                       nil,
	}
	for in, want := range cases {
		var m soloistMsg
		if err := json.Unmarshal([]byte(in), &m); err != nil {
			t.Fatal(err)
		}
		got := translate(m)
		if len(got) != len(want) || (len(got) > 0 && got[0] != want[0]) {
			t.Errorf("%s: got %v, want %v", in, got, want)
		}
	}
}

// recorder logs calls as "METHOD /path body" (ampd) or the raw JSON command (Soloist).
type recorder struct {
	mu    sync.Mutex
	calls []string
}

func (r *recorder) add(s string) {
	r.mu.Lock()
	r.calls = append(r.calls, s)
	r.mu.Unlock()
}

// has waits briefly, since Soloist commands arrive asynchronously over the WebSocket.
func (r *recorder) has(substr string) bool {
	for range 50 {
		r.mu.Lock()
		for _, c := range r.calls {
			if strings.Contains(c, substr) {
				r.mu.Unlock()
				return true
			}
		}
		r.mu.Unlock()
		time.Sleep(10 * time.Millisecond)
	}
	return false
}

func fakeAmp(t *testing.T, r *recorder) string {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
		b, _ := io.ReadAll(req.Body)
		r.add(req.Method + " " + req.URL.Path + " " + strings.TrimSpace(string(b)))
		json.NewEncoder(w).Encode(ampStatus{Power: true, Volume: 20, Input: 3})
	}))
	t.Cleanup(s.Close)
	return s.URL
}

func fakeSoloist(t *testing.T, r *recorder) string {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
		conn, err := websocket.Accept(w, req, nil)
		if err != nil {
			return
		}
		defer conn.CloseNow()
		for {
			_, data, err := conn.Read(req.Context())
			if err != nil {
				return
			}
			r.add(string(data))
		}
	}))
	t.Cleanup(s.Close)
	return "ws" + strings.TrimPrefix(s.URL, "http")
}

func newTestController(t *testing.T) (*controller, *recorder, *recorder) {
	amp, sol := &recorder{}, &recorder{}
	cfg := config{usbInput: 3, maxVolume: 50, idle: time.Minute}
	s := &soloist{url: fakeSoloist(t, sol)}
	ctx, cancel := context.WithCancel(context.Background())
	t.Cleanup(cancel)
	go s.events(ctx, make(chan event, 32))
	for range 100 { // wait for the WebSocket to connect
		s.mu.Lock()
		ok := s.conn != nil
		s.mu.Unlock()
		if ok {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	c := newController(cfg, &ampClient{fakeAmp(t, amp), http.DefaultClient}, s)
	return c, amp, sol
}

var ctx = context.Background()

func TestInputChangedWhilePlayingPausesSpotify(t *testing.T) {
	c, _, sol := newTestController(t)
	c.st, c.playing = ampStatus{Power: true, Volume: 20, Input: 3}, true
	c.onAmp(ctx, polled{st: ampStatus{Power: true, Volume: 20, Input: 1}, started: time.Now()})
	if !sol.has(`"command":"pause"`) {
		t.Fatal("expected pause when input switched to vinyl")
	}
	if !c.idleSince.IsZero() {
		t.Fatal("idle timer must not run for another source")
	}
}

func TestSinglePollErrorIsIgnored(t *testing.T) {
	c, _, _ := newTestController(t)
	c.st, c.playing = ampStatus{Power: true, Input: 3}, true
	c.onAmp(ctx, polled{err: errors.New("timeout"), started: time.Now()})
	if !c.owned(c.st) || !c.playing {
		t.Fatal("one failed poll must not count as amp off")
	}
	c.onAmp(ctx, polled{err: errors.New("timeout"), started: time.Now()})
	if c.owned(c.st) || c.playing {
		t.Fatal("two failed polls means amp off: expected pause")
	}
}

func TestStalePollIsDropped(t *testing.T) {
	c, _, _ := newTestController(t)
	c.st, c.playing = ampStatus{Power: true, Input: 3}, true
	started := time.Now()
	c.lastWrite.Store(time.Now().Add(time.Millisecond).UnixNano())
	c.onAmp(ctx, polled{st: ampStatus{Power: false}, started: started})
	if !c.owned(c.st) || !c.playing {
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

func TestSnapshotIdleDoesNotStartTimer(t *testing.T) {
	c, _, _ := newTestController(t)
	c.onEvent(ctx, event{kind: "idle"}) // playback_state sent on every (re)connect
	if !c.idleSince.IsZero() {
		t.Fatal("idle without prior playback must not start the power-off timer")
	}
	c.playing = true
	c.onEvent(ctx, event{kind: "paused"})
	if c.idleSince.IsZero() {
		t.Fatal("pause after playback must start the timer")
	}
}

func TestSpotifyVolumeIgnoredOnOtherSource(t *testing.T) {
	c, _, _ := newTestController(t)
	c.st = ampStatus{Power: true, Volume: 20, Input: 1}
	c.onEvent(ctx, event{kind: "volume", volume: 90})
	select {
	case <-c.volSignal:
		t.Fatal("Spotify volume must not reach the amp while TV/Vinyl is selected")
	default:
	}
}

func TestClaimSwitchesInputAndSyncsSlider(t *testing.T) {
	c, amp, sol := newTestController(t)
	c.st = ampStatus{Power: true, Volume: 20, Input: 1}
	c.onEvent(ctx, event{kind: "playing"})
	if !amp.has(`POST /input {"input":3}`) {
		t.Fatal("expected switch to USB input")
	}
	if amp.has("POST /power") {
		t.Fatal("amp already on: no power command expected")
	}
	if !sol.has(`"volume":40`) { // amp 20 of max 50
		t.Fatal("expected Spotify slider set from amp volume")
	}
}

func TestVolumeAfterPlayStartIgnored(t *testing.T) {
	c, _, sol := newTestController(t)
	c.st = ampStatus{Power: true, Volume: 20, Input: 3} // already playing Spotify
	c.onEvent(ctx, event{kind: "playing"})              // e.g. resume, or Soloist restarted
	c.onEvent(ctx, event{kind: "volume", volume: 100})  // WirePlumber restore / app's remembered level
	select {
	case <-c.volSignal:
		t.Fatal("volume event right after playback start must not reach the amp")
	default:
	}
	if c.st.Volume != 20 {
		t.Fatalf("amp volume = %d, want 20", c.st.Volume)
	}
	if !sol.has(`"volume":40`) {
		t.Fatal("expected Spotify slider put back to the amp level (40%)")
	}

	c.ignoreVol = time.Time{} // window over: the user moves the slider
	c.onEvent(ctx, event{kind: "volume", volume: 50})
	select {
	case <-c.volSignal:
	default:
		t.Fatal("slider moves after the window must reach the amp")
	}
	if c.st.Volume != 25 {
		t.Fatalf("amp volume = %d, want 25", c.st.Volume)
	}
}
