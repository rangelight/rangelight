package session

import (
	"context"
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
	// Namespace is the runner's own namespace.
	Namespace string
}

// runnerAccount is the runner's ServiceAccount in its namespace (the chart's templates/runner/rbac.yaml).
const runnerAccount = "runner"

// NewServer serves /auth/git and /healthz, with timeouts and a header cap: every request on it is
// either the mirror's auth subrequest or a probe.
func NewServer(addr string, auth *GitAuth) *http.Server {
	mux := http.NewServeMux()
	mux.Handle("GET /auth/git", auth)
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
