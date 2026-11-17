package main

import (
	"context"
	"log"
	"math"
	"os"
	"sync/atomic"
	"time"
)

// spotifyMax is Soloist's volume scale: 0-100.
const spotifyMax = 100

type config struct {
	ampURL     string
	soloistURL string
	usbInput   int           // amp input wired to the USB DAC
	maxVolume  int           // amp level that Spotify 100% maps to
	idle       time.Duration // power off after this long without Spotify playback
	poll       time.Duration
	card       string // /proc/asound/<card> to wait for after power on; empty skips
	stream     string // Soloist's PipeWire stream to keep at 100%; empty disables
}

// polled is one amp status poll; started lets the loop drop polls that overlap our own writes.
type polled struct {
	st      ampStatus
	err     error
	started time.Time
}

// controller owns all state; only run's goroutine touches it, except the atomics.
type controller struct {
	cfg config
	amp *ampClient
	sol *soloist
	pin *streamPinner // nil when cfg.stream is empty

	st        ampStatus // last known amp state
	pollErrs  int
	playing   bool
	idleSince time.Time
	ignoreVol time.Time // drop Spotify volume events until then (session start, our own echo)

	pendingVol atomic.Int64
	volSignal  chan struct{}
	lastWrite  atomic.Int64 // unix nanos of last completed amp write
}

func newController(cfg config, amp *ampClient, sol *soloist) *controller {
	c := &controller{cfg: cfg, amp: amp, sol: sol, volSignal: make(chan struct{}, 1)}
	if cfg.stream != "" {
		c.pin = newStreamPinner(cfg.stream)
	}
	return c
}

func (c *controller) run(ctx context.Context) {
	events := make(chan event, 32)
	statuses := make(chan polled, 4)
	go c.sol.events(ctx, events)
	go c.pollAmp(ctx, statuses)
	go c.volumeWriter(ctx)
	if c.pin != nil {
		go c.pin.run(ctx)
	}
	idle := time.NewTicker(30 * time.Second)
	defer idle.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case ev := <-events:
			c.onEvent(ctx, ev)
		case p := <-statuses:
			c.onAmp(ctx, p)
		case <-idle.C:
			c.checkIdle(ctx)
		}
	}
}

func (c *controller) owned(s ampStatus) bool { return s.Power && s.Input == c.cfg.usbInput }

func (c *controller) onEvent(ctx context.Context, ev event) {
	switch ev.kind {
	case "active":
		log.Print("spotify: active")
		c.claim(ctx)
	case "buffering", "playing":
		c.pin.poke() // new stream may start at Soloist's own volume
		c.playing = true
		if !c.owned(c.st) {
			c.claim(ctx)
		}
	case "paused", "idle", "inactive":
		// Only a stop after playback starts the timer; the on-connect snapshot must not.
		if c.playing {
			c.idleSince = time.Now()
		}
		c.playing = false
	case "volume":
		c.pin.poke() // Soloist applies the slider to its stream; undo that
		if time.Now().Before(c.ignoreVol) || !c.owned(c.st) {
			return
		}
		level := toAmp(ev.volume, spotifyMax, c.cfg.maxVolume)
		if level == c.st.Volume {
			return // echo of our own amp->Spotify sync
		}
		c.st.Volume = level
		c.pendingVol.Store(int64(level))
		select {
		case c.volSignal <- struct{}{}:
		default: // writer already signalled; it reads the latest value
		}
	}
}

// claim makes the amp play Spotify: power on, select USB input, show amp volume in Spotify.
func (c *controller) claim(ctx context.Context) {
	if !c.owned(c.st) {
		log.Printf("amp: claiming (power=%v input=%d)", c.st.Power, c.st.Input)
		if !c.st.Power {
			if err := c.amp.setPower(ctx, true); err != nil {
				log.Printf("amp: power on: %v", err)
				return
			}
			c.waitCard(ctx)
		}
		var err error
		for range 5 { // amp may ignore input changes while it is still powering up
			if err = c.amp.setInput(ctx, c.cfg.usbInput); err == nil {
				break
			}
			time.Sleep(time.Second)
		}
		c.lastWrite.Store(time.Now().UnixNano())
		if err != nil {
			log.Printf("amp: set input %d: %v", c.cfg.usbInput, err)
			return
		}
		c.st.Power, c.st.Input = true, c.cfg.usbInput
		c.idleSince = time.Now() // device picked but nothing played: still power off later
		if st, err := c.amp.status(ctx); err == nil {
			c.st = st
		}
	}
	c.syncSpotify(ctx)
}

func (c *controller) waitCard(ctx context.Context) {
	if c.cfg.card == "" {
		return
	}
	for range 20 {
		if _, err := os.Stat("/proc/asound/" + c.cfg.card); err == nil {
			return
		}
		select {
		case <-ctx.Done():
			return
		case <-time.After(500 * time.Millisecond):
		}
	}
	log.Printf("amp: sound card %q did not appear", c.cfg.card)
}

// syncSpotify sets the Spotify slider to the amp's volume.
func (c *controller) syncSpotify(ctx context.Context) {
	c.ignoreVol = time.Now().Add(2 * time.Second) // covers the session's initial volume event
	if err := c.sol.setVolume(ctx, toSpotify(c.st.Volume, spotifyMax, c.cfg.maxVolume)); err != nil {
		log.Printf("spotify: set volume: %v", err)
	}
}

func (c *controller) onAmp(ctx context.Context, p polled) {
	if p.started.UnixNano() < c.lastWrite.Load() {
		return // poll overlapped our own write and may be stale
	}
	st := p.st
	if p.err != nil {
		// ampd errors when the amp is in standby; one error may be a glitch.
		if c.pollErrs++; c.pollErrs < 2 {
			return
		}
		st = ampStatus{}
	} else {
		c.pollErrs = 0
	}
	was, now := c.owned(c.st), c.owned(st)
	volChanged := st.Volume != c.st.Volume
	c.st = st
	switch {
	case was && !now && c.playing:
		log.Printf("amp: off or input changed (power=%v input=%d), pausing Spotify", st.Power, st.Input)
		if err := c.sol.pause(ctx); err != nil {
			log.Printf("spotify: pause: %v", err)
		}
		c.playing = false
		c.idleSince = time.Time{} // someone else uses the amp now; never auto power off for them
	case was && now && volChanged:
		c.syncSpotify(ctx)
	}
}

func (c *controller) checkIdle(ctx context.Context) {
	if c.playing || c.idleSince.IsZero() || time.Since(c.idleSince) < c.cfg.idle {
		return
	}
	c.idleSince = time.Time{}
	if !c.owned(c.st) {
		return // amp is off or on another source (Vinyl, TV)
	}
	log.Printf("amp: Spotify idle for %v, powering off", c.cfg.idle)
	if err := c.amp.setPower(ctx, false); err != nil {
		log.Printf("amp: power off: %v", err)
		return
	}
	c.lastWrite.Store(time.Now().UnixNano())
	c.st.Power = false
}

func (c *controller) pollAmp(ctx context.Context, out chan<- polled) {
	t := time.NewTicker(c.cfg.poll)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
		}
		if time.Since(time.Unix(0, c.lastWrite.Load())) < 2*time.Second {
			continue // leave the serial line to volume writes while the slider moves
		}
		started := time.Now()
		st, err := c.amp.status(ctx)
		out <- polled{st, err, started}
	}
}

// volumeWriter sends the latest pending volume, at most one write per 100 ms.
func (c *controller) volumeWriter(ctx context.Context) {
	for {
		select {
		case <-ctx.Done():
			return
		case <-c.volSignal:
		}
		if err := c.amp.setVolume(ctx, int(c.pendingVol.Load())); err != nil {
			log.Printf("amp: set volume: %v", err)
		}
		c.lastWrite.Store(time.Now().UnixNano())
		time.Sleep(100 * time.Millisecond)
	}
}

// toAmp maps Spotify volume (0..max) to an amp level on a square curve, capped at ampMax.
func toAmp(v, max, ampMax int) int {
	f := math.Min(math.Max(float64(v)/float64(max), 0), 1)
	return int(math.Round(float64(ampMax) * f * f))
}

// toSpotify is the inverse of toAmp; amp levels above ampMax show as 100%.
func toSpotify(a, max, ampMax int) int {
	f := math.Min(math.Max(float64(a)/float64(ampMax), 0), 1)
	return int(math.Round(float64(max) * math.Sqrt(f)))
}
