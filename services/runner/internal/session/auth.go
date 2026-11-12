package session

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"regexp"
	"slices"
	"strings"
	"time"

	authnv1 "k8s.io/api/authentication/v1"
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/types"
	"k8s.io/client-go/util/retry"
	"sigs.k8s.io/controller-runtime/pkg/client"
)

const reviewTimeout = 5 * time.Second

var sessionID = regexp.MustCompile(`^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$`)

// GitAuth answers the git mirror's nginx auth_request (services/git-mirror, RAN-17). The caller
// shows a projected token, as a Basic password or a Bearer token, valid for the mirror's audience
// and bound to a pod (the API server checks both):
//   - the runner's own ServiceAccount in its namespace: X-Remote-User: runner (read-only, RAN-17);
//   - the agent pod of a live lane: X-Remote-User: session-<id>.
//
// Anything else is 401. It fails closed: no answer from the API server is a 401 too.
type GitAuth struct {
	// Client creates TokenReviews.
	Client client.Client
	// Cache holds lane namespaces, lane pods and Sessions (NewManager).
	Cache client.Reader
	// Reader reads past the cache: a push report updates the Session as the API server has it.
	Reader client.Reader
	// Namespace is the runner's own namespace.
	Namespace string
}

// runnerAccount is the runner's ServiceAccount in its namespace (the chart's templates/runner/rbac.yaml).
const runnerAccount = "runner"

// NewServer serves /auth/git, /git/pushed and /healthz, with timeouts and a header cap: every
// request on it is the mirror's auth subrequest, its push hook, or a probe.
func NewServer(addr string, auth *GitAuth) *http.Server {
	mux := http.NewServeMux()
	mux.Handle("GET /auth/git", auth)
	mux.HandleFunc("POST /git/pushed", auth.pushed)
	mux.HandleFunc("GET /healthz", func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(http.StatusOK) })
	return &http.Server{
		Addr: addr, Handler: mux, MaxHeaderBytes: 16 << 10,
		ReadHeaderTimeout: 5 * time.Second, WriteTimeout: 10 * time.Second, IdleTimeout: 60 * time.Second,
	}
}

// ServeHTTP implements http.Handler.
func (a *GitAuth) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	id, err := a.identify(r.Context(), r)
	if err != nil {
		// The reason only, never the token.
		slog.InfoContext(r.Context(), "git auth refused", "reason", err.Error())
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}
	w.Header().Set("X-Remote-User", id)
	w.WriteHeader(http.StatusOK)
}

// commitID is a SHA-1 or SHA-256 object id.
var commitID = regexp.MustCompile(`^[0-9a-f]{40}([0-9a-f]{24})?$`)

// pushed is the git mirror's post-receive hook telling the runner that a session pushed its work
// branch. The hook forwards the pushing session's own token, which is identified exactly as for
// /auth/git: the session is that identity's, and the request may only confirm it. It records the
// commit as the Session's status.workCommit, which makes the reconciler publish the branch, so a
// publish that is due survives a restart. 202: recorded.
func (a *GitAuth) pushed(w http.ResponseWriter, r *http.Request) {
	id, err := a.identify(r.Context(), r)
	if err != nil {
		slog.InfoContext(r.Context(), "git push report refused", "reason", err.Error())
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}
	session, ok := strings.CutPrefix(id, "session-")
	if !ok {
		http.Error(w, "only a session pushes", http.StatusForbidden)
		return
	}
	var push struct{ Session, Ref, Commit string }
	if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4<<10)).Decode(&push); err != nil {
		http.Error(w, "want {session, ref, commit}", http.StatusBadRequest)
		return
	}
	if push.Session != session || push.Ref != "refs/heads/"+workBranch(session) || !commitID.MatchString(push.Commit) {
		slog.InfoContext(r.Context(), "git push report refused", "session_id", session, "reason", "not this session's work branch")
		http.Error(w, "only your own work branch", http.StatusForbidden)
		return
	}
	if err := a.recordPush(r.Context(), session, push.Commit); err != nil {
		slog.WarnContext(r.Context(), "git push report not recorded", "session_id", session, "error", err.Error())
		http.Error(w, "not recorded", http.StatusServiceUnavailable)
		return
	}
	w.WriteHeader(http.StatusAccepted)
}

// recordPush sets the Session's workCommit and counts the push (status.pushes), optimistically
// locked so two reports at once both count: each push, even of a commit seen before, is published.
// The cache finds the Session; every read of it for the update is the API server's, so a retry
// after a conflict never sees the stale copy again.
func (a *GitAuth) recordPush(ctx context.Context, id, commit string) error {
	cached, err := byID(ctx, a.Cache, id)
	if err != nil || cached == nil {
		return fmt.Errorf("session %s: not found", id)
	}
	err = retry.RetryOnConflict(retry.DefaultRetry, func() error {
		var s Session
		if err := a.Reader.Get(ctx, client.ObjectKeyFromObject(cached), &s); err != nil {
			return fmt.Errorf("session %s: %w", id, err)
		}
		before := s.DeepCopy()
		s.Status.WorkCommit = commit
		s.Status.Pushes++
		return a.Client.Status().Patch(ctx, &s, client.MergeFromWithOptions(before, client.MergeFromWithOptimisticLock{}))
	})
	if err != nil {
		return fmt.Errorf("push report: %w", err)
	}
	return nil
}

func (a *GitAuth) identify(ctx context.Context, r *http.Request) (string, error) {
	token := bearer(r)
	if token == "" {
		return "", errors.New("no token")
	}
	user, err := a.review(ctx, token)
	if err != nil {
		return "", err
	}
	ns, name, ok := serviceAccount(user.Username)
	if !ok {
		return "", errors.New("not a service account")
	}
	if ns == a.Namespace && name == runnerAccount {
		if one(user.Extra["authentication.kubernetes.io/pod-name"]) == "" {
			return "", errors.New("the runner's token is not bound to a pod")
		}
		return runnerAccount, nil
	}
	var lane corev1.Namespace
	if err := a.Cache.Get(ctx, client.ObjectKey{Name: ns}, &lane); err != nil {
		return "", fmt.Errorf("namespace %s is not a lane: %w", ns, err)
	}
	id := lane.Labels[LabelSession]
	if !sessionID.MatchString(id) {
		return "", fmt.Errorf("namespace %s has no session id", ns)
	}
	// The label alone is a claim: the namespace must be the lane of the live Session with that id.
	s, err := byID(ctx, a.Cache, id)
	if err != nil || s == nil || Namespace(s) != ns || !s.DeletionTimestamp.IsZero() {
		return "", fmt.Errorf("namespace %s is not the lane of session %s", ns, id)
	}
	if err := a.agentPod(ctx, ns, id, user.Extra); err != nil {
		return "", fmt.Errorf("service account %s/%s: %w", ns, name, err)
	}
	return "session-" + id, nil
}

// review asks the API server whether the token is valid for the git mirror's audience.
func (a *GitAuth) review(ctx context.Context, token string) (authnv1.UserInfo, error) {
	ctx, cancel := context.WithTimeout(ctx, reviewTimeout)
	defer cancel()
	tr := &authnv1.TokenReview{Spec: authnv1.TokenReviewSpec{Token: token, Audiences: []string{MirrorAudience}}}
	if err := a.Client.Create(ctx, tr); err != nil {
		return authnv1.UserInfo{}, fmt.Errorf("token review: %w", err)
	}
	if !tr.Status.Authenticated || !slices.Contains(tr.Status.Audiences, MirrorAudience) {
		return authnv1.UserInfo{}, errors.New("token not valid for " + MirrorAudience)
	}
	return tr.Status.User, nil
}

// agentPod checks that the token is bound to the lane's agent pod, not to another pod or to none.
func (a *GitAuth) agentPod(ctx context.Context, ns, id string, extra map[string]authnv1.ExtraValue) error {
	name, uid := one(extra["authentication.kubernetes.io/pod-name"]), one(extra["authentication.kubernetes.io/pod-uid"])
	if name != agentName || uid == "" {
		return errors.New("token is not bound to the agent pod")
	}
	var pod corev1.Pod
	if err := a.Cache.Get(ctx, client.ObjectKey{Namespace: ns, Name: name}, &pod); err != nil {
		return fmt.Errorf("agent pod: %w", err)
	}
	if pod.UID != types.UID(uid) || pod.Labels[LabelSession] != id {
		return errors.New("token is bound to another pod")
	}
	return nil
}

// bearer is the token from "Authorization: Bearer <token>" or the password of Basic auth.
func bearer(r *http.Request) string {
	if _, pass, ok := r.BasicAuth(); ok {
		return pass
	}
	if token, ok := strings.CutPrefix(r.Header.Get("Authorization"), "Bearer "); ok {
		return strings.TrimSpace(token)
	}
	return ""
}

// serviceAccount splits system:serviceaccount:<namespace>:<name>.
func serviceAccount(user string) (string, string, bool) {
	rest, ok := strings.CutPrefix(user, "system:serviceaccount:")
	ns, name, found := strings.Cut(rest, ":")
	return ns, name, ok && found && ns != "" && name != ""
}

func one(v authnv1.ExtraValue) string {
	if len(v) != 1 {
		return ""
	}
	return v[0]
}
