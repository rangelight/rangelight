package session

import (
	"context"
	"fmt"
	"log/slog"
	"math/rand/v2"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
	"sigs.k8s.io/controller-runtime/pkg/manager"
)

// The runner protocol (api/README.md, ADR 0021): Sessions arrive as SessionDesired on the runner's
// desired subject and their phases go back as SessionStatus on its status subject. Lanes never
// depend on it: the reconciler runs whether NATS is up or not.

const (
	protocolVersion = "v1alpha1"
	// FieldManager owns the Session spec the runner applies (server-side apply).
	FieldManager = "rangelight-runner"
	minBackoff   = 500 * time.Millisecond
	maxBackoff   = 30 * time.Second
	// natsTimeout bounds every request to NATS.
	natsTimeout = 5 * time.Second
	// ackWait is how long a delivered message is the runner's; applyTimeout bounds handling it, below.
	ackWait      = 30 * time.Second
	applyTimeout = 20 * time.Second
	// slowDeliveries is how many deliveries of one message are worth a warning.
	slowDeliveries = 5
)

// DesiredSubject is the JetStream subject of one session's SessionDesired at a runner. org and runner
// are subject tokens (internal/config checks them), session a UUID. The stream keeps the last message
// of each: a session's desired state, and a cold one is its tombstone.
func DesiredSubject(org, runner, session string) string {
	return "rl." + org + ".runner." + runner + ".desired." + session
}

// StatusSubject is the JetStream subject a runner reports SessionStatus on.
func StatusSubject(org, runner string) string { return "rl." + org + ".runner." + runner + ".status" }

// backoff is the pause before retry number attempt (from 0): exponential from 500ms, capped at 30s,
// with jitter in its upper half so runners don't retry in step.
func backoff(attempt int) time.Duration {
	d := min(maxBackoff, minBackoff<<min(attempt, 16))
	return d/2 + rand.N(d/2+1) //nolint:gosec // jitter, not a secret
}

// sleep waits d, or until ctx ends; it reports whether ctx is still live.
func sleep(ctx context.Context, d time.Duration) bool {
	t := time.NewTimer(d)
	defer t.Stop()
	select {
	case <-ctx.Done():
		return false
	case <-t.C:
		return true
	}
}

// AddProtocol connects to NATS at url (in the background: an unreachable server is retried with
// backoff for ever) and adds to mgr the desired-session consumer and the status publisher. It
// returns the reconciler's Report.
func AddProtocol(mgr manager.Manager, url, namespace, org, runner string) (func(*Session), error) {
	nc, err := nats.Connect(url, nats.Name(FieldManager+" "+runner), nats.Timeout(natsTimeout),
		nats.RetryOnFailedConnect(true), nats.MaxReconnects(-1),
		nats.CustomReconnectDelay(func(attempts int) time.Duration { return backoff(attempts - 1) }),
		nats.ConnectHandler(func(*nats.Conn) { slog.Info("nats connected", "url", url) }),
		nats.ReconnectHandler(func(*nats.Conn) { slog.Info("nats reconnected", "url", url) }),
		nats.DisconnectErrHandler(func(_ *nats.Conn, err error) { slog.Warn("nats disconnected", "url", url, "error", fmt.Sprint(err)) }))
	if err != nil {
		return nil, fmt.Errorf("nats: %w", err)
	}
	js, err := jetstream.New(nc)
	if err != nil {
		nc.Close()
		return nil, fmt.Errorf("jetstream: %w", err)
	}
	status := newStatus(js, org, runner)
	desired := &desired{Client: mgr.GetClient(), Reader: mgr.GetAPIReader(), js: js, namespace: namespace, runner: runner, subject: DesiredSubject(org, runner, "*")}
	if err := mgr.Add(manager.RunnableFunc(func(ctx context.Context) error {
		defer nc.Close()
		go status.run(ctx)
		desired.run(ctx)
		return nil
	})); err != nil {
		nc.Close()
		return nil, fmt.Errorf("protocol: %w", err)
	}
	return status.report, nil
}
