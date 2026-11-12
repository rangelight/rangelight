package session

import (
	"context"
	"fmt"
	"log/slog"
	"strings"
	"sync"

	"github.com/nats-io/nats.go/jetstream"
	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/types/known/timestamppb"

	runnerv1 "github.com/rangelight/rangelight/shared/go/gen/rangelight/runner/v1alpha1"
)

// phases maps a Session's phase to the runner protocol's.
var phases = map[Phase]runnerv1.Phase{
	PhasePending:  runnerv1.Phase_PHASE_QUEUED,
	PhaseStarting: runnerv1.Phase_PHASE_STARTING,
	PhaseHot:      runnerv1.Phase_PHASE_RUNNING,
	PhaseFailed:   runnerv1.Phase_PHASE_FAILED,
	PhaseGone:     runnerv1.Phase_PHASE_GONE,
}

// status publishes each Session's phase as a SessionStatus, with Nats-Msg-Id and message_id set to
// the phase's PhaseID. The reconciler hands it every Session it settles (report, which never
// blocks); it keeps the latest unsent report per session and publishes it, retrying with backoff
// while NATS is away. After a restart it publishes each Session's current phase once more: the
// same message, which JetStream and the api drop as a repeat.
type status struct {
	js              jetstream.JetStream
	subject, runner string
	wake            chan struct{}
	mu              sync.Mutex
	pending         map[string]*runnerv1.Envelope // the latest unsent report, by session id
	sent            map[string]string             // the last published PhaseID, by session id
}

func newStatus(js jetstream.JetStream, org, runner string) *status {
	return &status{
		js: js, subject: StatusSubject(org, runner), runner: runner, wake: make(chan struct{}, 1),
		pending: map[string]*runnerv1.Envelope{}, sent: map[string]string{},
	}
}

// report queues s's phase, unless it was already published.
func (p *status) report(s *Session) {
	phase, known := phases[s.Status.Phase]
	if !known || s.Status.PhaseID == "" {
		return
	}
	id := s.Spec.ID
	p.mu.Lock()
	if p.sent[id] != s.Status.PhaseID {
		p.pending[id] = &runnerv1.Envelope{
			MessageId: s.Status.PhaseID, ProtocolVersion: protocolVersion, RunnerId: p.runner,
			Body: &runnerv1.Envelope_Status{Status: &runnerv1.SessionStatus{
				SessionId: id, ObservedGeneration: uint64(max(0, s.Spec.Generation)), Phase: phase, Message: strings.TrimSpace(s.Status.Message + " " + s.Status.PublishMessage),
				PullRequestUrl: s.Status.PullRequestURL,
			}},
		}
	}
	p.mu.Unlock()
	select {
	case p.wake <- struct{}{}:
	default:
	}
}

// run publishes until ctx ends.
func (p *status) run(ctx context.Context) {
	attempt := 0
	for {
		select {
		case <-ctx.Done():
			return
		case <-p.wake:
		}
		for {
			err := p.flush(ctx)
			if err == nil {
				attempt = 0
				break
			}
			wait := backoff(attempt)
			attempt++
			slog.WarnContext(ctx, "runner protocol: status not published; retrying", "error", err.Error(), "retry_in", wait.String())
			if !sleep(ctx, wait) {
				return
			}
		}
	}
}

// flush publishes every pending report, each acknowledged by JetStream before it is dropped.
func (p *status) flush(ctx context.Context) error {
	for {
		p.mu.Lock()
		var id string
		var env *runnerv1.Envelope
		for id, env = range p.pending {
			break
		}
		p.mu.Unlock()
		if env == nil {
			return nil
		}
		if err := p.publish(ctx, env); err != nil {
			return err
		}
		p.mu.Lock()
		if p.pending[id] == env { // not replaced by a newer report meanwhile
			delete(p.pending, id)
			p.sent[id] = env.GetMessageId()
		}
		if env.GetStatus().GetPhase() == runnerv1.Phase_PHASE_GONE {
			delete(p.sent, id) // the session's last report
		}
		p.mu.Unlock()
	}
}

func (p *status) publish(ctx context.Context, env *runnerv1.Envelope) error {
	env.SentAt = timestamppb.Now()
	data, err := proto.Marshal(env)
	if err != nil {
		return fmt.Errorf("status: %w", err)
	}
	ctx, cancel := context.WithTimeout(ctx, natsTimeout)
	defer cancel()
	if _, err := p.js.Publish(ctx, p.subject, data, jetstream.WithMsgID(env.GetMessageId())); err != nil {
		return fmt.Errorf("status %s: %w", env.GetStatus().GetSessionId(), err)
	}
	return nil
}
