package session

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/nats-io/nats.go/jetstream"
	"google.golang.org/protobuf/proto"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/watch"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/yaml"

	runnerv1 "github.com/rangelight/rangelight/shared/go/gen/rangelight/runner/v1alpha1"
)

// The subjects are the ones api/README.md documents.
func TestSubjectsAreTheContract(t *testing.T) {
	readme, err := os.ReadFile("../../../../api/README.md")
	if err != nil {
		t.Fatal(err)
	}
	for _, subject := range []string{DesiredSubject("<org>", "<runner>", "<session>"), StatusSubject("<org>", "<runner>")} {
		if !strings.Contains(string(readme), "`"+subject+"`") {
			t.Errorf("api/README.md doesn't document %s", subject)
		}
	}
}

// AC4: retries back off exponentially from 500ms to 30s, with jitter in the upper half.
func TestBackoffIsCappedWithJitter(t *testing.T) {
	for attempt := range 40 {
		ceiling := min(maxBackoff, minBackoff<<min(attempt, 16))
		if d := backoff(attempt); d < ceiling/2 || d > ceiling {
			t.Errorf("attempt %d: %s, want %s to %s", attempt, d, ceiling/2, ceiling)
		}
	}
	if backoff(40) < maxBackoff/2 {
		t.Error("the cap fell")
	}
}

// AC6: the CRD's ticket schema is api/jsonschema/ticket.v1alpha1.schema.json, keyword for keyword
// (less what OpenAPI doesn't take: annotations, additionalProperties, and the CRD's CEL rules).
func TestTicketSchemaIsTheContract(t *testing.T) {
	var crd struct {
		Spec struct {
			Versions []struct {
				Schema struct {
					OpenAPIV3Schema struct {
						Properties map[string]struct {
							Properties map[string]any `json:"properties"`
						} `json:"properties"`
					} `json:"openAPIV3Schema"`
				} `json:"schema"`
			} `json:"versions"`
		} `json:"spec"`
	}
	raw, err := os.ReadFile(chart + "/files/crds/session.yaml")
	if err == nil {
		err = yaml.Unmarshal(raw, &crd)
	}
	if err != nil {
		t.Fatal(err)
	}
	var contract any
	raw, err = os.ReadFile("../../../../api/jsonschema/ticket.v1alpha1.schema.json")
	if err == nil {
		err = json.Unmarshal(raw, &contract)
	}
	if err != nil {
		t.Fatal(err)
	}
	got := structural(crd.Spec.Versions[0].Schema.OpenAPIV3Schema.Properties["spec"].Properties["ticket"])
	if want := structural(contract); !reflect.DeepEqual(got, want) {
		t.Errorf("files/crds/session.yaml spec.ticket:\n%v\nwant (from the ticket schema):\n%v", got, want)
	}
}

// structural is a schema without its annotations and the keywords only one side has.
func structural(v any) any {
	switch v := v.(type) {
	case map[string]any:
		out := map[string]any{}
		for k, sub := range v {
			switch k {
			case "$schema", "$id", "$comment", "title", "description", "additionalProperties", "x-kubernetes-validations":
			case "properties": // names, not keywords
				props := map[string]any{}
				for name, schema := range sub.(map[string]any) {
					props[name] = structural(schema)
				}
				out[k] = props
			default:
				out[k] = structural(sub)
			}
		}
		return out
	case []any:
		out := make([]any, len(v))
		for i, sub := range v {
			out[i] = structural(sub)
		}
		return out
	}
	return v
}

// ticketKeys gives each session of these tests its own ticket key.
var ticketKeys = map[string]string{}

// desiredMsg is a SessionDesired for session id on repo, as the relay publishes it.
func desiredMsg(id, repo string, generation uint64, state runnerv1.DesiredState, title string) *runnerv1.Envelope {
	if ticketKeys[id] == "" {
		ticketKeys[id] = fmt.Sprintf("PROTO-%d", len(ticketKeys)+1)
	}
	return &runnerv1.Envelope{
		MessageId: uuid.Must(uuid.NewV7()).String(), ProtocolVersion: protocolVersion, RunnerId: runnerID,
		Body: &runnerv1.Envelope_Desired{Desired: &runnerv1.SessionDesired{
			SessionId: id, Generation: generation, State: state, Member: "owner",
			Ticket:    &runnerv1.Ticket{Key: ticketKeys[id], Title: title, Criteria: []string{"AC1 it works"}},
			Worktrees: []*runnerv1.Worktree{{MirrorRepo: repo, BaseRef: "main"}},
		}},
	}
}

// publish sends a message on its session's desired subject at its runner and returns its stream
// sequence.
func publish(t *testing.T, env *runnerv1.Envelope) uint64 {
	t.Helper()
	data, err := proto.Marshal(env)
	if err != nil {
		t.Fatal(err)
	}
	return publishRaw(t, DesiredSubject(org, env.GetRunnerId(), env.GetDesired().GetSessionId()), data)
}

func publishRaw(t *testing.T, subject string, data []byte) uint64 {
	t.Helper()
	js, nc, err := broker.jetstream()
	if err != nil {
		t.Fatal(err)
	}
	defer nc.Close()
	ack, err := js.Publish(context.Background(), subject, data, jetstream.WithMsgID(uuid.NewString()))
	if err != nil {
		t.Fatal(err)
	}
	return ack.Sequence
}

// handled waits until the runner has acknowledged stream message seq, and checks it wasn't
// terminated instead.
func handled(t *testing.T, seq uint64) {
	t.Helper()
	settled(t, seq)
	time.Sleep(200 * time.Millisecond) // a termination's advisory follows it
	if broker.terminated(seq) {
		t.Errorf("message %d was terminated, want it acknowledged", seq)
	}
}

// termed waits until the runner has terminated stream message seq: refused, never redelivered.
func termed(t *testing.T, seq uint64) {
	t.Helper()
	eventually(t, fmt.Sprintf("message %d terminated", seq), func() error {
		if !broker.terminated(seq) {
			return errors.New("not terminated")
		}
		return nil
	})
}

// settled waits until the runner's consumer is past stream message seq.
func settled(t *testing.T, seq uint64) {
	t.Helper()
	eventually(t, fmt.Sprintf("message %d handled", seq), func() error {
		js, nc, err := broker.jetstream()
		if err != nil {
			return err
		}
		defer nc.Close()
		c, err := js.Consumer(context.Background(), DesiredStream(org), "runner-"+runnerID)
		if err != nil {
			return err
		}
		info, err := c.Info(context.Background())
		if err != nil {
			return err
		}
		if info.AckFloor.Stream < seq {
			return fmt.Errorf("acknowledged up to %d", info.AckFloor.Stream)
		}
		return nil
	})
}

// statuses are the SessionStatus envelopes the runner published for session id with this phase,
// each checked to carry its message_id as its Nats-Msg-Id.
func statuses(id string, want runnerv1.Phase) ([]*runnerv1.Envelope, error) {
	js, nc, err := broker.jetstream()
	if err != nil {
		return nil, err
	}
	defer nc.Close()
	c, err := js.OrderedConsumer(context.Background(), "rl-"+org+"-status", jetstream.OrderedConsumerConfig{FilterSubjects: []string{StatusSubject(org, runnerID)}})
	if err != nil {
		return nil, err
	}
	batch, err := c.FetchNoWait(1000)
	if err != nil {
		return nil, err
	}
	var found []*runnerv1.Envelope
	for m := range batch.Messages() {
		env := &runnerv1.Envelope{}
		if proto.Unmarshal(m.Data(), env) != nil || env.GetStatus().GetSessionId() != id || env.GetStatus().GetPhase() != want {
			continue
		}
		if got := m.Headers().Get(jetstream.MsgIDHeader); got != env.GetMessageId() {
			return nil, fmt.Errorf("Nats-Msg-Id %q isn't message_id %q", got, env.GetMessageId())
		}
		found = append(found, env)
	}
	return found, nil
}

// reported waits until the runner has published exactly one SessionStatus with this phase for
// session id, and returns it.
func reported(t *testing.T, id string, want runnerv1.Phase) *runnerv1.Envelope {
	t.Helper()
	var found []*runnerv1.Envelope
	eventually(t, "status "+want.String(), func() error {
		var err error
		if found, err = statuses(id, want); err == nil && len(found) != 1 {
			err = fmt.Errorf("%d reports", len(found))
		}
		return err
	})
	return found[0]
}

func session(t *testing.T, id string) *Session {
	t.Helper()
	s := &Session{ObjectMeta: metav1.ObjectMeta{Name: id, Namespace: runnerNS}}
	get(t, s)
	return s
}

// AC5, end to end: SessionDesired -> the Session (applied by the runner's field manager) -> its pod
// Ready -> RUNNING reported with the Session's phase id -> a cold SessionDesired (teardown) -> the
// Session gone, and GONE reported.
func TestAC5DesiredToRunningToGone(t *testing.T) {
	demoRepo(t, "proto")
	id := uuid.Must(uuid.NewV7()).String()
	handled(t, publish(t, desiredMsg(id, "proto", 1, runnerv1.DesiredState_DESIRED_STATE_HOT, "Over NATS")))
	s := session(t, id)
	if s.Spec.Generation != 1 || s.Spec.Ticket.Title != "Over NATS" || s.Spec.Repo != "proto" || s.Spec.Member != "owner" {
		t.Errorf("spec %+v", s.Spec)
	}
	if !slices.ContainsFunc(s.ManagedFields, func(f metav1.ManagedFieldsEntry) bool {
		return f.Manager == FieldManager && f.Operation == metav1.ManagedFieldsOperationApply
	}) {
		t.Errorf("not applied by %s: %+v", FieldManager, s.ManagedFields)
	}
	ensureServiceAccount(t, Namespace(s))
	pod := &corev1.Pod{ObjectMeta: metav1.ObjectMeta{Name: agentName, Namespace: Namespace(s)}}
	get(t, pod)
	setAgentStatus(t, pod, true, corev1.ContainerState{Running: &corev1.ContainerStateRunning{}}, nil)
	hot := phase(t, s, PhaseHot)
	running := reported(t, id, runnerv1.Phase_PHASE_RUNNING)
	if running.GetMessageId() != hot.Status.PhaseID || running.GetStatus().GetObservedGeneration() != 1 {
		t.Errorf("RUNNING %v, want message_id %s and generation 1", running, hot.Status.PhaseID)
	}
	handled(t, publish(t, desiredMsg(id, "proto", 2, runnerv1.DesiredState_DESIRED_STATE_COLD, "Over NATS")))
	gone(t, s)
	reported(t, id, runnerv1.Phase_PHASE_GONE)
}

// AC1: a message that can never apply is terminated, not redelivered for ever, and doesn't hold up
// the next one; an older generation changes nothing.
func TestAC1BadAndStaleMessagesAreDropped(t *testing.T) {
	demoRepo(t, "stale")
	termed(t, publishRaw(t, DesiredSubject(org, runnerID, uuid.NewString()), []byte("not protobuf")))
	invalid := desiredMsg(uuid.Must(uuid.NewV7()).String(), "stale", 1, runnerv1.DesiredState_DESIRED_STATE_HOT, "")
	termed(t, publish(t, invalid)) // an empty title breaks the ticket rule: the API server refuses it
	if err := k8s.Get(context.Background(), client.ObjectKey{Namespace: runnerNS, Name: invalid.GetDesired().GetSessionId()}, &Session{}); err == nil {
		t.Error("an invalid ticket became a Session")
	}
	other := desiredMsg(uuid.Must(uuid.NewV7()).String(), "stale", 1, runnerv1.DesiredState_DESIRED_STATE_HOT, "t")
	other.RunnerId = "someone-else"
	termed(t, publishRaw(t, DesiredSubject(org, runnerID, other.GetDesired().GetSessionId()), mustMarshal(t, other)))
	// A message on another session's subject would overwrite that session's desired state.
	termed(t, publishRaw(t, DesiredSubject(org, runnerID, uuid.NewString()), mustMarshal(t, desiredMsg(uuid.Must(uuid.NewV7()).String(), "stale", 1, runnerv1.DesiredState_DESIRED_STATE_HOT, "t"))))

	id := uuid.Must(uuid.NewV7()).String()
	handled(t, publish(t, desiredMsg(id, "stale", 3, runnerv1.DesiredState_DESIRED_STATE_HOT, "Three")))
	handled(t, publish(t, desiredMsg(id, "stale", 2, runnerv1.DesiredState_DESIRED_STATE_HOT, "Two")))
	if s := session(t, id); s.Spec.Generation != 3 || s.Spec.Ticket.Title != "Three" {
		t.Errorf("an older generation applied: %+v", s.Spec)
	}
	handled(t, publish(t, desiredMsg(id, "stale", 4, runnerv1.DesiredState_DESIRED_STATE_HOT, "Four")))
	handled(t, publish(t, desiredMsg(id, "stale", 5, runnerv1.DesiredState_DESIRED_STATE_WARM, "Warm")))
	s := session(t, id)
	if s.Spec.Ticket.Title != "Four" || s.Spec.Generation != 4 {
		t.Errorf("want generation 4 kept (a newer one applies, a warm one changes nothing): %+v", s.Spec)
	}
	handled(t, publish(t, desiredMsg(id, "stale", 6, runnerv1.DesiredState_DESIRED_STATE_COLD, "Four")))
	gone(t, s)
}

func mustMarshal(t *testing.T, env *runnerv1.Envelope) []byte {
	t.Helper()
	data, err := proto.Marshal(env)
	if err != nil {
		t.Fatal(err)
	}
	return data
}

// Review B1: the desired stream is the desired-state table. A session that went hot then cold, read
// by a brand-new consumer (a runner starting over), is never created; a still-hot one is.
func TestB1ANewRunnerNeverRevivesAClosedSession(t *testing.T) {
	demoRepo(t, "replay")
	const fresh = "r2"
	closed, open := uuid.Must(uuid.NewV7()).String(), uuid.Must(uuid.NewV7()).String()
	for _, env := range []*runnerv1.Envelope{
		desiredMsg(closed, "replay", 1, runnerv1.DesiredState_DESIRED_STATE_HOT, "Closed"),
		desiredMsg(open, "replay", 1, runnerv1.DesiredState_DESIRED_STATE_HOT, "Open"),
		desiredMsg(closed, "replay", 2, runnerv1.DesiredState_DESIRED_STATE_COLD, "Closed"),
	} {
		env.RunnerId = fresh
		publish(t, env)
	}
	events, err := watcher.Watch(context.Background(), &SessionList{}, client.InNamespace(runnerNS))
	if err != nil {
		t.Fatal(err)
	}
	defer events.Stop()
	js, nc, err := broker.jetstream()
	if err != nil {
		t.Fatal(err)
	}
	defer nc.Close()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	d := &desired{Client: asRunner, Reader: asRunner, js: js, namespace: runnerNS, runner: fresh, stream: DesiredStream(org), subject: DesiredSubject(org, fresh, "*")}
	go d.run(ctx)
	s := session(t, open)
	cancel()
	for drained := false; !drained; {
		select {
		case e := <-events.ResultChan():
			if s, ok := e.Object.(*Session); ok && e.Type == watch.Added && s.Name == closed {
				t.Errorf("the closed session %s was created again", closed)
			}
		case <-time.After(time.Second):
			drained = true
		}
	}
	if err := k8s.Delete(context.Background(), s); err != nil {
		t.Fatal(err)
	}
	gone(t, s)
}

// AC4: with NATS down, a lane still comes up (the reconciler never waits on NATS), and once NATS is
// back the runner reconnects: the queued status goes out and new sessions arrive again.
func TestAC4LanesRunWhileNATSIsDown(t *testing.T) {
	demoRepo(t, "outage")
	broker.Stop()
	restarted := false
	defer func() {
		if !restarted {
			_ = broker.Start()
		}
	}()
	s := newSession(t, "outage")
	starting := phase(t, s, PhaseStarting)
	time.Sleep(time.Second) // a few failed publishes
	if err := broker.Start(); err != nil {
		t.Fatal(err)
	}
	restarted = true
	if got := reported(t, s.Spec.ID, runnerv1.Phase_PHASE_STARTING); got.GetMessageId() != starting.Status.PhaseID {
		t.Errorf("STARTING %v, want message_id %s", got, starting.Status.PhaseID)
	}
	id := uuid.Must(uuid.NewV7()).String()
	handled(t, publish(t, desiredMsg(id, "outage", 1, runnerv1.DesiredState_DESIRED_STATE_HOT, "After the outage")))
	session(t, id)
	handled(t, publish(t, desiredMsg(id, "outage", 2, runnerv1.DesiredState_DESIRED_STATE_COLD, "After the outage")))
}
