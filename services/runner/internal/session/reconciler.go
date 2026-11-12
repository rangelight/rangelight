package session

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
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
)

const messageMax = 2000

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
	result, uerr := r.settle(ctx, &s, before)
	return result, errors.Join(err, uerr)
}

// settle writes s's status if it changed from before, with a new phase id for a new phase or
// message (so the api learns why a lane is stuck), and reports it. Optimistically locked: a stale read must not mint a second id for one phase change.
func (r *Reconciler) settle(ctx context.Context, s, before *Session) (ctrl.Result, error) {
	if s.Status.Phase != before.Status.Phase || s.Status.Message != before.Status.Message || s.Status.PhaseID == "" {
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
	ticket, err := json.Marshal(s.Spec.Ticket)
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

// oneLine is s on one line, at most messageMax characters (the CRD's maxLength counts characters),
// never cut inside one.
func oneLine(s string) string {
	s = strings.Join(strings.Fields(s), " ")
	if r := []rune(s); len(r) > messageMax {
		s = string(r[:messageMax])
	}
	return s
}
