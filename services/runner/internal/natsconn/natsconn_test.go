package natsconn

import (
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	natsserver "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"
)

// tokens signs in every client and records the token each connect presented.
type tokens struct {
	mu   sync.Mutex
	seen []string
}

func (a *tokens) Check(c natsserver.ClientAuthentication) bool {
	a.mu.Lock()
	defer a.mu.Unlock()
	token := c.GetOpts().Token
	a.seen = append(a.seen, token)
	if token == "bad" {
		return false
	}
	c.RegisterUser(&natsserver.User{Username: "runner"})
	return true
}

func (a *tokens) count(token string) int {
	a.mu.Lock()
	defer a.mu.Unlock()
	n := 0
	for _, t := range a.seen {
		if t == token {
			n++
		}
	}
	return n
}

func (a *tokens) list() []string {
	a.mu.Lock()
	defer a.mu.Unlock()
	return append([]string(nil), a.seen...)
}

// The token is read from its file at every connect, so a rotated one is used at the next; replies
// come under the runner's own inbox.
func TestTheTokenIsReadAtEveryConnect(t *testing.T) {
	auth := &tokens{}
	s, err := natsserver.NewServer(&natsserver.Options{Host: "127.0.0.1", Port: -1, NoSigs: true, NoLog: true, CustomClientAuthentication: auth})
	if err != nil {
		t.Fatal(err)
	}
	go s.Start()
	defer s.Shutdown()
	if !s.ReadyForConnections(10 * time.Second) {
		t.Fatal("nats not ready")
	}
	file := filepath.Join(t.TempDir(), "token")
	if err := os.WriteFile(file, []byte("first\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	reconnected := make(chan struct{}, 1)
	opts := append(Options(file, "r1"), nats.ReconnectHandler(func(*nats.Conn) { reconnected <- struct{}{} }))
	nc, err := nats.Connect(s.ClientURL(), opts...)
	if err != nil {
		t.Fatal(err)
	}
	defer nc.Close()
	if err := os.WriteFile(file, []byte("rotated"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := nc.ForceReconnect(); err != nil {
		t.Fatal(err)
	}
	select {
	case <-reconnected:
	case <-time.After(10 * time.Second):
		t.Fatal("no reconnect")
	}
	if got := auth.list(); strings.Join(got, ",") != "first,rotated" {
		t.Errorf("tokens %v, want first then rotated", got)
	}
	if inbox := nc.NewRespInbox(); !strings.HasPrefix(inbox, "_INBOX_runner-r1.") {
		t.Errorf("inbox %s", inbox)
	}
}

// Refused sign-ins in a row (the callout away, a bad token) never stop the runner's reconnecting: once
// the token is good again it is back.
func TestRefusedSignInsKeepReconnecting(t *testing.T) {
	auth := &tokens{}
	s, err := natsserver.NewServer(&natsserver.Options{Host: "127.0.0.1", Port: -1, NoSigs: true, NoLog: true, CustomClientAuthentication: auth})
	if err != nil {
		t.Fatal(err)
	}
	go s.Start()
	defer s.Shutdown()
	if !s.ReadyForConnections(10 * time.Second) {
		t.Fatal("nats not ready")
	}
	file := filepath.Join(t.TempDir(), "token")
	if err := os.WriteFile(file, []byte("good"), 0o600); err != nil {
		t.Fatal(err)
	}
	reconnected := make(chan struct{}, 1)
	opts := append(Options(file, "r1"), nats.MaxReconnects(-1), nats.ReconnectWait(20*time.Millisecond),
		nats.ReconnectHandler(func(*nats.Conn) { reconnected <- struct{}{} }))
	nc, err := nats.Connect(s.ClientURL(), opts...)
	if err != nil {
		t.Fatal(err)
	}
	defer nc.Close()
	if err := os.WriteFile(file, []byte("bad"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := nc.ForceReconnect(); err != nil {
		t.Fatal(err)
	}
	for deadline := time.Now().Add(10 * time.Second); auth.count("bad") < 3; time.Sleep(10 * time.Millisecond) {
		if time.Now().After(deadline) {
			t.Fatalf("refused %d times, want 3", auth.count("bad"))
		}
	}
	if err := os.WriteFile(file, []byte("good"), 0o600); err != nil {
		t.Fatal(err)
	}
	select {
	case <-reconnected:
	case <-time.After(10 * time.Second):
		t.Fatalf("not back after the refusals; closed: %v", nc.IsClosed())
	}
}
