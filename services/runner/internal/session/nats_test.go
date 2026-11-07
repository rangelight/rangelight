package session

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"os"
	"sync"
	"time"

	natsserver "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
)

// testNATS is a real nats-server with JetStream on a fixed local port, so it can be stopped and
// started again (an outage) at the same URL, with its streams kept on disk.
type testNATS struct {
	URL    string
	dir    string
	opts   natsserver.Options
	server *natsserver.Server
	// advisories hears JetStream's MSG_TERMINATED advisories; terms holds their stream sequences.
	advisories *nats.Conn
	mu         sync.Mutex
	terms      map[uint64]bool
}

func newTestNATS() (*testNATS, error) {
	dir, err := os.MkdirTemp("", "rl-nats-")
	if err != nil {
		return nil, fmt.Errorf("nats: %w", err)
	}
	n := &testNATS{dir: dir, terms: map[uint64]bool{}, opts: natsserver.Options{Host: "127.0.0.1", Port: -1, JetStream: true, StoreDir: dir, NoSigs: true, NoLog: true}}
	if err := n.Start(); err != nil {
		return nil, err
	}
	n.opts.Port = n.server.Addr().(*net.TCPAddr).Port
	if err := n.streams(); err != nil {
		return nil, err
	}
	return n, n.listen()
}

// listen records every message a desired consumer terminates.
func (n *testNATS) listen() error {
	nc, err := nats.Connect(n.URL, nats.MaxReconnects(-1), nats.ReconnectWait(100*time.Millisecond))
	if err != nil {
		return fmt.Errorf("nats: %w", err)
	}
	n.advisories = nc
	_, err = nc.Subscribe("$JS.EVENT.ADVISORY.CONSUMER.MSG_TERMINATED.rl-"+org+"-desired.*", func(m *nats.Msg) {
		var advisory struct {
			StreamSeq uint64 `json:"stream_seq"`
		}
		if json.Unmarshal(m.Data, &advisory) == nil {
			n.mu.Lock()
			n.terms[advisory.StreamSeq] = true
			n.mu.Unlock()
		}
	})
	if err == nil {
		err = nc.Flush()
	}
	if err != nil {
		return fmt.Errorf("advisories: %w", err)
	}
	return nil
}

// terminated reports whether a desired consumer terminated stream message seq.
func (n *testNATS) terminated(seq uint64) bool {
	n.mu.Lock()
	defer n.mu.Unlock()
	return n.terms[seq]
}

// Start starts the server (again).
func (n *testNATS) Start() error {
	opts := n.opts
	s, err := natsserver.NewServer(&opts)
	if err != nil {
		return fmt.Errorf("nats: %w", err)
	}
	go s.Start()
	if !s.ReadyForConnections(10 * time.Second) {
		return fmt.Errorf("nats: not ready")
	}
	n.server, n.URL = s, s.ClientURL()
	return nil
}

// Stop stops the server, keeping its data.
func (n *testNATS) Stop() {
	n.server.Shutdown()
	n.server.WaitForShutdown()
}

// Close stops the server and removes its data.
func (n *testNATS) Close() {
	n.advisories.Close()
	n.Stop()
	_ = os.RemoveAll(n.dir)
}

// streams creates the org's streams as the relay (desired: one message per session) and the api
// (status) do.
func (n *testNATS) streams() error {
	js, nc, err := n.jetstream()
	if err != nil {
		return err
	}
	defer nc.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	for _, cfg := range []jetstream.StreamConfig{
		{Name: "rl-" + org + "-desired", Subjects: []string{DesiredSubject(org, "*", "*")}, MaxMsgsPerSubject: 1, Duplicates: 2 * time.Minute},
		{Name: "rl-" + org + "-status", Subjects: []string{StatusSubject(org, "*")}, Duplicates: 2 * time.Minute},
	} {
		if _, err := js.CreateStream(ctx, cfg); err != nil {
			return fmt.Errorf("stream %s: %w", cfg.Name, err)
		}
	}
	return nil
}

// jetstream is a client of the server, as the control plane.
func (n *testNATS) jetstream() (jetstream.JetStream, *nats.Conn, error) {
	nc, err := nats.Connect(n.URL)
	if err != nil {
		return nil, nil, fmt.Errorf("nats: %w", err)
	}
	js, err := jetstream.New(nc)
	if err != nil {
		nc.Close()
		return nil, nil, fmt.Errorf("jetstream: %w", err)
	}
	return js, nc, nil
}
