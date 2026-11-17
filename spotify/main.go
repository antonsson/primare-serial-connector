// spotd syncs Spotify Soloist (Spotify Connect) with the Primare I22 via ampd.
package main

import (
	"context"
	"flag"
	"log"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"
)

func main() {
	var cfg config
	flag.StringVar(&cfg.ampURL, "amp", "http://127.0.0.1:3000", "ampd base URL")
	flag.StringVar(&cfg.soloistURL, "soloist", "ws://127.0.0.1:9090", "Soloist WebSocket URL (soloist --ws)")
	flag.IntVar(&cfg.usbInput, "usb-input", 0, "amp input (1-7) wired to the USB DAC (required)")
	flag.IntVar(&cfg.maxVolume, "max-volume", 50, "amp level (0-79) that Spotify 100% maps to")
	flag.DurationVar(&cfg.idle, "idle", 20*time.Minute, "power off the amp after Spotify is idle this long")
	flag.DurationVar(&cfg.poll, "poll", time.Second, "amp status poll interval")
	flag.StringVar(&cfg.card, "card", "", "ALSA card id to wait for after power on, as in /proc/asound/<card>")
	flag.StringVar(&cfg.stream, "stream", "spotify", "Soloist's PipeWire stream (application or node name, case-insensitive) to keep at 100% volume so only the amp sets loudness; empty disables")
	flag.Parse()
	if cfg.usbInput < 1 || cfg.usbInput > 7 {
		log.Fatal("-usb-input must be 1-7")
	}
	if cfg.maxVolume < 1 || cfg.maxVolume > 79 {
		log.Fatal("-max-volume must be 1-79")
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	client := &http.Client{Timeout: 5 * time.Second}
	c := newController(cfg, &ampClient{cfg.ampURL, client}, &soloist{url: cfg.soloistURL})
	log.Printf("spotd: amp=%s soloist=%s usb-input=%d max-volume=%d stream=%q", cfg.ampURL, cfg.soloistURL, cfg.usbInput, cfg.maxVolume, cfg.stream)
	c.run(ctx)
}
