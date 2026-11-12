package session

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/google/uuid"
	corev1 "k8s.io/api/core/v1"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/api/meta"
	"k8s.io/apimachinery/pkg/api/resource"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/controller/controllerutil"

	"github.com/rangelight/rangelight/services/runner/internal/github"
)

const (
	messageMax = 2000
	// mirrorTimeout bounds the call that removes a closed session's repo from the git mirror.
	mirrorTimeout = 10 * time.Second
)

// Options are the runner's settings that shape a lane (internal/config).
type Options struct {
	// AgentImage is the lane agent's image, by tag and digest.
	AgentImage string
	// MirrorURL is the git mirror's base URL; MirrorTokenFile holds the runner's own projected token
	// for it, read at each fetch.
	MirrorURL, MirrorTokenFile string
	// Workspace, Tmp and EphemeralStorage bound the agent pod's disk: the worktree's and /tmp's
	// emptyDirs, and the container's ephemeral-storage limit.
	Workspace, Tmp, EphemeralStorage resource.Quantity
	// DNS selects the cluster DNS pods, the lane's only DNS server.
	DNS DNS
	// GitHub, if set, publishes each session's work branch to Upstreams[Spec.Repo], a git URL
	// whose path is /<owner>/<repo>.git, and opens its pull request.
	GitHub    *github.App
	Upstreams map[string]string
	// PublishImage is the git mirror's image, whose publish.sh each publish Job runs.
	PublishImage string
	// Workers is how many Sessions reconcile at once, so one slow mirror fetch stalls only one.
	Workers int
}

// Reconciler converges each Session to its lane, and removes the lane when the Session is deleted.
type Reconciler struct {
	client.Client
	// Reader reads past the cache, where a cache miss would be taken for "not there".
	Reader client.Reader
	// Report, if set, is handed every Session whose status the reconciler settled, and the Session
	// that was deleted, with phase Gone (AddProtocol's status publisher). It must not block.
	Report func(*Session)
	Options
	mirrorHost string
	profiles   *profiles
	// mirrorGiveUp is how long after its deletion a Session waits for the mirror to drop its repo.
	mirrorGiveUp time.Duration
	pub          *publishing
}

// Reconcile implements reconcile.Reconciler.
func (r *Reconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	var s Session
	if err := r.Get(ctx, req.NamespacedName, &s); apierrors.IsNotFound(err) {
		return ctrl.Result{}, nil
	} else if err != nil {
		return ctrl.Result{}, fmt.Errorf("session %s: %w", req.NamespacedName, err)
	}
	if !s.DeletionTimestamp.IsZero() {
		return ctrl.Result{}, r.remove(ctx, &s)
	}
	if controllerutil.AddFinalizer(&s, finalizer) {
		if err := r.Update(ctx, &s); err != nil {
			return ctrl.Result{}, fmt.Errorf("session %s: add finalizer: %w", s.Spec.ID, err)
		}
	}
	if s.Status.Phase == PhaseFailed {
		r.report(&s)
		return ctrl.Result{}, nil
	}
	before := s.DeepCopy()
	if s.Status.Phase == "" {
		s.Status.Phase = PhasePending
	}
	phase, err := r.converge(ctx, &s)
	var f *failedError
	if errors.As(err, &f) {
		slog.WarnContext(ctx, "session failed", "session_id", s.Spec.ID, "reason", f.msg)
		s.Status.Phase, s.Status.Message, err = PhaseFailed, oneLine(f.msg), nil
	} else if phase != "" {
		s.Status.Phase = phase
	}
	var wait time.Duration
	if err == nil && s.Status.Phase != PhaseFailed {
		wait = r.publishDue(ctx, &s)
	}
	result, uerr := r.settle(ctx, &s, before)
	if wait > 0 && result.RequeueAfter == 0 {
		result.RequeueAfter = wait
	}
	return result, errors.Join(err, uerr)
}

// settle writes s's status if it changed from before, with a new phase id for a new phase or
// message (so the api learns why a lane is stuck) or pull request, and reports it. Optimistically locked: a stale read must not mint a second id for one phase change.
func (r *Reconciler) settle(ctx context.Context, s, before *Session) (ctrl.Result, error) {
	if s.Status.Phase != before.Status.Phase || s.Status.Message != before.Status.Message ||
		s.Status.PullRequestURL != before.Status.PullRequestURL || s.Status.PublishMessage != before.Status.PublishMessage ||
		s.Status.PhaseID == "" {
		id, err := uuid.NewV7()
		if err != nil {
			return ctrl.Result{}, fmt.Errorf("session %s: phase id: %w", s.Spec.ID, err)
		}
		s.Status.PhaseID = id.String()
	}
	if s.Status != before.Status {
		err := r.Status().Patch(ctx, s, client.MergeFromWithOptions(before, client.MergeFromWithOptimisticLock{}))
		if apierrors.IsConflict(err) {
			return ctrl.Result{RequeueAfter: time.Second}, nil
		}
		if err != nil {
			return ctrl.Result{}, fmt.Errorf("session %s: status: %w", s.Spec.ID, err)
		}
	}
	r.report(s)
	return ctrl.Result{}, nil
}

func (r *Reconciler) report(s *Session) {
	if r.Report != nil {
		r.Report(s)
	}
}

// converge brings the lane up in order and returns the phase it reached ("" before the namespace).
func (r *Reconciler) converge(ctx context.Context, s *Session) (Phase, error) {
	config, err := r.laneConfig(ctx, s)
	if err != nil {
		return "", err
	}
	if err := r.ensureNamespace(ctx, s); err != nil {
		return "", err
	}
	if err := r.ensurePolicies(ctx, s); err != nil {
		return PhaseStarting, err
	}
	if err := r.createOnce(ctx, claudeHomeClaim(s), &corev1.PersistentVolumeClaim{}); err != nil {
		return PhaseStarting, err
	}
	data, labels := config.Data, config.Labels
	if _, err := controllerutil.CreateOrUpdate(ctx, r.Client, config, func() error {
		config.Data, config.Labels = data, labels
		return nil
	}); err != nil {
		return PhaseStarting, fmt.Errorf("lane %s: config: %w", Namespace(s), err)
	}
	var pod corev1.Pod
	err = r.Get(ctx, client.ObjectKey{Namespace: Namespace(s), Name: agentName}, &pod)
	switch {
	case apierrors.IsNotFound(err):
		return PhaseStarting, r.createOnce(ctx, agentPod(s, r.Options), &corev1.Pod{})
	case err != nil:
		return PhaseStarting, fmt.Errorf("lane %s: agent pod: %w", Namespace(s), err)
	}
	phase, waiting, err := podPhase(&pod)
	s.Status.Message = oneLine(waiting)
	return phase, err
}

// laneConfig is the lane's ConfigMap: the ticket, and the profile read once from the base branch.
func (r *Reconciler) laneConfig(ctx context.Context, s *Session) (*corev1.ConfigMap, error) {
	t := s.Spec.Ticket
	t.URL = "" // the agent doesn't need it
	ticket, err := json.Marshal(t)
	if err != nil {
		return nil, fmt.Errorf("session %s: ticket: %w", s.Spec.ID, err)
	}
	var cm corev1.ConfigMap
	err = r.Reader.Get(ctx, client.ObjectKey{Namespace: Namespace(s), Name: laneName}, &cm)
	if err != nil && !apierrors.IsNotFound(err) {
		return nil, fmt.Errorf("lane %s: config: %w", Namespace(s), err)
	}
	// Only this session's own copy: an earlier session of the same ticket may have left one behind.
	var profile []byte
	if cm.Labels[LabelSession] == s.Spec.ID {
		profile = []byte(cm.Data["profile.json"])
	}
	if len(profile) == 0 {
		var commit string
		commit, profile, err = r.profiles.read(ctx, s.Spec.Repo, s.Spec.BaseRef)
		if commit != "" {
			s.Status.ProfileCommit = commit
		}
		if err != nil {
			return nil, err
		}
	}
	return laneConfig(s, ticket, profile), nil
}

// ensureNamespace creates the lane namespace, and never adopts one it didn't create for this Session:
// a namespace made in advance by someone else would hand them the lane's NATS identity (its terminal).
func (r *Reconciler) ensureNamespace(ctx context.Context, s *Session) error {
	want := namespace(s)
	var got corev1.Namespace
	err := r.Reader.Get(ctx, client.ObjectKey{Name: want.Name}, &got)
	switch {
	case apierrors.IsNotFound(err):
		if err := r.Create(ctx, want); err != nil {
			return fmt.Errorf("lane %s: create namespace: %w", want.Name, err)
		}
		return nil
	case err != nil:
		return fmt.Errorf("lane %s: namespace: %w", want.Name, err)
	case !got.DeletionTimestamp.IsZero():
		// An earlier session of the same ticket is still going away.
		return fmt.Errorf("lane %s: namespace is still being deleted", want.Name)
	case !owns(&got, s):
		return failed("namespace_taken: namespace %s already exists and is not this session's", want.Name)
	}
	return nil
}

// ensurePolicies writes both egress policies, and puts them back when they change (both are watched).
func (r *Reconciler) ensurePolicies(ctx context.Context, s *Session) error {
	np := egressPolicy(s)
	spec := np.Spec
	if _, err := controllerutil.CreateOrUpdate(ctx, r.Client, np, func() error { np.Spec = spec; return nil }); err != nil {
		return fmt.Errorf("lane %s: network policy: %w", Namespace(s), err)
	}
	cnp := ciliumPolicy(s, r.mirrorHost, r.DNS)
	cspec := cnp.Object["spec"]
	_, err := controllerutil.CreateOrUpdate(ctx, r.Client, cnp, func() error { cnp.Object["spec"] = cspec; return nil })
	if meta.IsNoMatchError(err) {
		return failed("CiliumNetworkPolicy is not installed: a lane reaches Anthropic only through Cilium (ADR 0046)")
	}
	if err != nil {
		return fmt.Errorf("lane %s: cilium policy: %w", Namespace(s), err)
	}
	return nil
}

// createOnce creates obj unless it exists. into is an empty object of obj's type.
func (r *Reconciler) createOnce(ctx context.Context, obj, into client.Object) error {
	err := r.Get(ctx, client.ObjectKeyFromObject(obj), into)
	if apierrors.IsNotFound(err) {
		err = r.Create(ctx, obj)
	}
	if err != nil && !apierrors.IsAlreadyExists(err) {
		return fmt.Errorf("lane %s: %s: %w", obj.GetNamespace(), obj.GetName(), err)
	}
	return nil
}

// remove deletes the lane namespace, and with it everything in the lane, then lets the Session go.
func (r *Reconciler) remove(ctx context.Context, s *Session) error {
	if !controllerutil.ContainsFinalizer(s, finalizer) {
		return nil
	}
	var ns corev1.Namespace
	err := r.Reader.Get(ctx, client.ObjectKey{Name: Namespace(s)}, &ns)
	switch {
	case apierrors.IsNotFound(err):
	case err != nil:
		return fmt.Errorf("lane %s: namespace: %w", Namespace(s), err)
	case owns(&ns, s) && ns.DeletionTimestamp.IsZero():
		if err := r.Delete(ctx, &ns); client.IgnoreNotFound(err) != nil {
			return fmt.Errorf("lane %s: delete namespace: %w", Namespace(s), err)
		}
	}
	if err := r.revokeRunning(ctx, s); err != nil {
		return err
	}
	if err := r.removeMirrorRepo(ctx, s.Spec.ID); err != nil {
		if time.Since(s.DeletionTimestamp.Time) < r.mirrorGiveUp {
			return err
		}
		slog.WarnContext(ctx, "the git mirror didn't remove a closed session's repo; giving up, it stays",
			"session_id", s.Spec.ID, "repo", "/srv/git/sessions/"+s.Spec.ID+".git", "error", err.Error())
	}
	controllerutil.RemoveFinalizer(s, finalizer)
	if err := r.Update(ctx, s); err != nil {
		return fmt.Errorf("session %s: remove finalizer: %w", s.Spec.ID, err)
	}
	// The Session is gone with its status, so this report lives only in memory.
	if id, err := uuid.NewV7(); err == nil {
		s.Status = Status{Phase: PhaseGone, PhaseID: id.String()}
		r.report(s)
	}
	return nil
}

// owns reports whether ns is the one the runner created for s.
func owns(ns *corev1.Namespace, s *Session) bool {
	return s.UID != "" && ns.Annotations[AnnotationSessionUID] == string(s.UID) && ns.Labels[LabelSession] == s.Spec.ID
}

// removeMirrorRepo asks the git mirror, as the runner, to drop the session's repo
// (DELETE /sessions/<id>.git; services/git-mirror/backend.sh). A repo already gone is fine. The path
// is the session id, which the CRD holds to a UUID, so no upstream repo is ever named. Until the
// mirror answers, the Session stays (its finalizer retries with the controller's backoff), for at
// most mirrorGiveUp after its deletion. Without a mirror (no MirrorURL) there is nothing to remove.
func (r *Reconciler) removeMirrorRepo(ctx context.Context, id string) error {
	if r.MirrorURL == "" {
		return nil
	}
	token, err := os.ReadFile(r.MirrorTokenFile)
	if err != nil {
		return fmt.Errorf("session %s: mirror token: %w", id, err)
	}
	ctx, cancel := context.WithTimeout(ctx, mirrorTimeout)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, r.MirrorURL+"/sessions/"+id+".git", nil)
	if err != nil {
		return fmt.Errorf("session %s: mirror repo: %w", id, err)
	}
	req.SetBasicAuth("runner", strings.TrimSpace(string(token)))
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return fmt.Errorf("session %s: remove the mirror repo: %w", id, err)
	}
	_ = resp.Body.Close()
	if resp.StatusCode != http.StatusNoContent && resp.StatusCode != http.StatusNotFound {
		return fmt.Errorf("session %s: remove the mirror repo: the mirror answered %d", id, resp.StatusCode)
	}
	return nil
}

// oneLine is s on one line, at most messageMax characters (the CRD's maxLength counts characters),
// never cut inside one.
func oneLine(s string) string {
	s = strings.Join(strings.Fields(s), " ")
	if r := []rune(s); len(r) > messageMax {
		s = string(r[:messageMax])
	}
	return s
}
