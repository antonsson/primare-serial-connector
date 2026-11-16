package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
)

// ampStatus is the subset of ampd's GET /status that spotd uses.
type ampStatus struct {
	Power  bool `json:"power"`
	Volume int  `json:"volume"` // 0-79
	Input  int  `json:"input"`  // 1-7
}

// ampClient talks to the ampd REST service (../service). Only this file knows its API.
type ampClient struct {
	base string
	http *http.Client
}

func (a *ampClient) status(ctx context.Context) (ampStatus, error) {
	var s ampStatus
	err := getJSON(ctx, a.http, a.base+"/status", &s)
	return s, err
}

func (a *ampClient) setPower(ctx context.Context, on bool) error {
	state := "off"
	if on {
		state = "on"
	}
	return postJSON(ctx, a.http, a.base+"/power", map[string]string{"state": state})
}

func (a *ampClient) setVolume(ctx context.Context, level int) error {
	return postJSON(ctx, a.http, a.base+"/volume", map[string]int{"level": level})
}

func (a *ampClient) setInput(ctx context.Context, input int) error {
	return postJSON(ctx, a.http, a.base+"/input", map[string]int{"input": input})
}

func getJSON(ctx context.Context, c *http.Client, url string, out any) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return err
	}
	resp, err := c.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode/100 != 2 {
		return fmt.Errorf("GET %s: %s", url, resp.Status)
	}
	return json.NewDecoder(resp.Body).Decode(out)
}

func postJSON(ctx context.Context, c *http.Client, url string, body any) error {
	var buf bytes.Buffer
	if body != nil {
		if err := json.NewEncoder(&buf).Encode(body); err != nil {
			return err
		}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, &buf)
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	resp, err := c.Do(req)
	if err != nil {
		return err
	}
	resp.Body.Close()
	if resp.StatusCode/100 != 2 {
		return fmt.Errorf("POST %s: %s", url, resp.Status)
	}
	return nil
}
