package session

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"math"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/nats-io/nats.go/jetstream"
	"google.golang.org/protobuf/proto"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
	"k8s.io/apimachinery/pkg/runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"

	runnerv1 "github.com/rangelight/rangelight/shared/go/gen/rangelight/runner/v1alpha1"
)

// desired is the runner's durable JetStream consumer of its sessions' SessionDesired, from the last
// message of each session's subject: a runner that starts over reads each session's latest desired
// state only, so a session that went hot then cold is never created again. Each message becomes
// its Session: applied (server-side apply, FieldManager) for a hot session, deleted for a cold one;
// the reconciler does the rest. A message is acknowledged only after that write succeeds, and
// terminated, with its reason logged, when it can never succeed. Dedupe needs no memory: JetStream
// drops a repeated Nats-Msg-Id, an apply is idempotent, and an older generation is ignored.
type desired struct {
	client.Client
	// Reader reads past the cache, so generations are compared with what the API server holds.
	Reader client.Reader
	js     jetstream.JetStream
	// stream is the org's desired stream (DesiredStream); subject filters every session's desired
	// subject at this runner (DesiredSubject(org, runner, "*")).
	namespace, runner, stream, subject string
}

// refused is a message that can never be applied.
type refused struct{ reason string }

func (r *refused) Error() string { return r.reason }

func refuse(format string, args ...any) error { return &refused{fmt.Sprintf(format, args...)} }

// run consumes until ctx ends, reaching the stream again with backoff whenever it is unavailable.
func (d *desired) run(ctx context.Context) {
	for attempt := 0; ; attempt++ {
		err := d.consume(ctx)
		if ctx.Err() != nil {
			return
		}
		wait := backoff(attempt)
		slog.WarnContext(ctx, "runner protocol: desired sessions unavailable", "error", err.Error(), "retry_in", wait.String())
		if !sleep(ctx, wait) {
			return
		}
	}
}

// consume creates the durable consumer (if missing) and handles messages until ctx ends or the
// consumer stops. One message in flight, so a session's messages apply in order.
func (d *desired) consume(ctx context.Context) error {
	setup, cancel := context.WithTimeout(ctx, natsTimeout)
	defer cancel()
	consumer, err := d.js.CreateOrUpdateConsumer(setup, d.stream, jetstream.ConsumerConfig{
		Durable: "runner-" + d.runner, FilterSubject: d.subject, DeliverPolicy: jetstream.DeliverLastPerSubjectPolicy,
		AckPolicy: jetstream.AckExplicitPolicy, MaxAckPending: 1, AckWait: ackWait,
	})
	if err != nil {
		return fmt.Errorf("consumer: %w", err)
	}
	consuming, err := consumer.Consume(func(m jetstream.Msg) { d.handle(ctx, m) })
	if err != nil {
		return fmt.Errorf("consume: %w", err)
	}
	defer consuming.Stop()
	select {
	case <-ctx.Done():
		return nil
	case <-consuming.Closed():
		return errors.New("the consumer stopped")
	}
}

func (d *desired) handle(ctx context.Context, m jetstream.Msg) {
	if meta, err := m.Metadata(); err == nil && meta.NumDelivered > slowDeliveries {
		slog.WarnContext(ctx, "runner protocol: a desired session keeps failing", "subject", m.Subject(), "deliveries", meta.NumDelivered)
	}
	// Bounded below AckWait, so a stuck API server call can't outlive the message's lease.
	applying, cancel := context.WithTimeout(ctx, applyTimeout)
	defer cancel()
	err := d.apply(applying, m.Subject(), m.Data())
	var bad *refused
	switch {
	case errors.As(err, &bad):
		slog.WarnContext(ctx, "runner protocol: refused a desired session", "reason", bad.reason)
		err = m.TermWithReason(bad.reason)
	case err != nil:
		slog.WarnContext(ctx, "runner protocol: couldn't apply a desired session; redelivering", "error", err.Error())
		err = m.NakWithDelay(time.Second)
	default:
		err = m.Ack()
	}
	if err != nil {
		slog.WarnContext(ctx, "runner protocol: couldn't acknowledge", "error", err.Error())
	}
}

// apply makes one SessionDesired, from subject, the Session's spec. A refused error means the
// message can never apply.
func (d *desired) apply(ctx context.Context, subject string, data []byte) error {
	var env runnerv1.Envelope
	if err := proto.Unmarshal(data, &env); err != nil {
		return refuse("undecodable")
	}
	want := env.GetDesired()
	switch {
	case env.GetProtocolVersion() != protocolVersion:
		return refuse("unsupported protocol %q", env.GetProtocolVersion())
	case env.GetRunnerId() != d.runner:
		return refuse("for runner %q", env.GetRunnerId())
	case want == nil:
		return refuse("not a SessionDesired")
	case want.GetGeneration() == 0 || want.GetGeneration() > math.MaxInt64:
		return refuse("generation %d", want.GetGeneration())
	}
	id, err := uuid.Parse(want.GetSessionId())
	if err != nil || id.String() != want.GetSessionId() {
		return refuse("session_id %q isn't a lowercase UUID", want.GetSessionId())
	}
	if !strings.HasSuffix(subject, ".desired."+id.String()) {
		return refuse("session %s on another session's subject %s", id, subject)
	}
	var current Session
	err = d.Reader.Get(ctx, client.ObjectKey{Namespace: d.namespace, Name: id.String()}, &current)
	if client.IgnoreNotFound(err) != nil {
		return fmt.Errorf("session %s: %w", id, err)
	}
	generation := int64(want.GetGeneration()) //nolint:gosec // checked against MaxInt64 above
	if err == nil && current.Spec.Generation >= generation {
		return nil // an older or repeated message: the Session already has this or newer
	}
	switch want.GetState() {
	case runnerv1.DesiredState_DESIRED_STATE_HOT:
		return d.applyHot(ctx, want, generation)
	case runnerv1.DesiredState_DESIRED_STATE_COLD:
		if apierrors.IsNotFound(err) {
			return nil
		}
		if err := d.Delete(ctx, &current); client.IgnoreNotFound(err) != nil {
			return fmt.Errorf("session %s: delete: %w", id, err)
		}
		return nil
	case runnerv1.DesiredState_DESIRED_STATE_WARM:
		// Hibernation (ADR 0010) isn't built: the lane stays hot.
		slog.InfoContext(ctx, "runner protocol: a warm session stays hot", "session_id", id.String())
		return nil
	case runnerv1.DesiredState_DESIRED_STATE_UNSPECIFIED:
		return refuse("no desired state")
	default:
		return refuse("desired state %v", want.GetState())
	}
}

// applyHot server-side applies the Session's spec. The API server validates it against the CRD
// (the ticket rule, the repo, base branch and member rules): an invalid spec is refused.
func (d *desired) applyHot(ctx context.Context, want *runnerv1.SessionDesired, generation int64) error {
	ticket, worktrees := want.GetTicket(), want.GetWorktrees()
	if ticket == nil || len(worktrees) != 1 {
		return refuse("session %s: want a ticket and one worktree", want.GetSessionId())
	}
	spec, err := runtime.DefaultUnstructuredConverter.ToUnstructured(&Spec{
		ID: want.GetSessionId(), Repo: worktrees[0].GetMirrorRepo(), BaseRef: worktrees[0].GetBaseRef(),
		Member: want.GetMember(), Generation: generation,
		Ticket: Ticket{Key: ticket.GetKey(), Title: ticket.GetTitle(), Description: ticket.GetDescription(), Criteria: ticket.GetCriteria(), URL: ticket.GetUrl()},
	})
	if err != nil {
		return fmt.Errorf("session %s: %w", want.GetSessionId(), err)
	}
	obj := &unstructured.Unstructured{Object: map[string]any{
		"apiVersion": GroupVersion.String(), "kind": "Session",
		"metadata": map[string]any{"name": want.GetSessionId(), "namespace": d.namespace},
		"spec":     spec,
	}}
	err = d.Apply(ctx, client.ApplyConfigurationFromUnstructured(obj), client.FieldOwner(FieldManager), client.ForceOwnership)
	if apierrors.IsInvalid(err) || apierrors.IsBadRequest(err) {
		return refuse("session %s: %s", want.GetSessionId(), oneLine(err.Error()))
	}
	if err != nil {
		return fmt.Errorf("session %s: apply: %w", want.GetSessionId(), err)
	}
	return nil
}
