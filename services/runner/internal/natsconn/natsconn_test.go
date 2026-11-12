package natsconn

import (
	"bufio"
	"fmt"
	"net"
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

// listen is a listener on a free local port, closed when the test ends.
func listen(t *testing.T) net.Listener {
	t.Helper()
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = l.Close() })
	return l
}

// serveDropping accepts n connections on l, each sent info, answered PONG once it pings, and then
// dropped; the returned channel says how that went.
func serveDropping(l net.Listener, info string, n int) <-chan error {
	served := make(chan error, 1)
	go func() {
		for range n {
			c, err := l.Accept()
			if err != nil {
				served <- err
				return
			}
			err = upThenDrop(c, info)
			if err != nil {
				served <- err
				return
			}
		}
		served <- nil
	}()
	return served
}

func upThenDrop(c net.Conn, info string) error {
	defer func() { _ = c.Close() }()
	if _, err := c.Write([]byte(info)); err != nil {
		return err
	}
	r := bufio.NewReader(c)
	for {
		line, err := r.ReadString('\n')
		if err != nil {
			return err
		}
		if strings.HasPrefix(line, "PING") {
			break
		}
	}
	if _, err := c.Write([]byte("PONG\r\n")); err != nil {
		return err
	}
	time.Sleep(200 * time.Millisecond)
	return nil
}

// RAN-80: a server that advertises another address (INFO connect_urls) and then drops the runner
// never gets it dialed: the runner reconnects to the configured server only.
func TestAReconnectNeverDialsAnAdvertisedServer(t *testing.T) {
	configured, advertised := listen(t), listen(t)
	dialed := make(chan struct{}, 1)
	go func() {
		if c, err := advertised.Accept(); err == nil {
			_ = c.Close()
			dialed <- struct{}{}
		}
	}()
	info := fmt.Sprintf("INFO {\"server_id\":\"fake\",\"version\":\"2.15.2\",\"max_payload\":1048576,\"connect_urls\":[%q]}\r\n",
		advertised.Addr().String())
	// Three connections from the runner: each is dropped once it is up.
	served := serveDropping(configured, info, 3)
	tokenFile := filepath.Join(t.TempDir(), "token")
	if err := os.WriteFile(tokenFile, []byte("t\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	opts := append(Options(tokenFile, "r1"), nats.ReconnectWait(50*time.Millisecond), nats.ReconnectJitter(0, 0))
	nc, err := nats.Connect("nats://"+configured.Addr().String(), opts...)
	if err != nil {
		t.Fatal(err)
	}
	defer nc.Close()
	select {
	case err := <-served:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(20 * time.Second):
		t.Fatal("the runner didn't reconnect to the configured server")
	}
	select {
	case <-dialed:
		t.Fatal("the runner dialed the advertised server")
	default:
	}
}
