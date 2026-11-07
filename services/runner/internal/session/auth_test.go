package session

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	authnv1 "k8s.io/api/authentication/v1"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
)

// token is a projected-style token for the namespace's default ServiceAccount, bound to pod when
// it is set, the way the kubelet requests the lane's token.
func token(t *testing.T, ns string, pod *corev1.Pod, audience string) string {
	t.Helper()
	ensureServiceAccount(t, ns)
	return saToken(t, ns, "default", pod, audience)
}

// saToken is a token for the ServiceAccount ns/sa, bound to pod when it is set.
func saToken(t *testing.T, ns, sa string, pod *corev1.Pod, audience string) string {
	t.Helper()
	expiry := int64(600)
	req := &authnv1.TokenRequest{Spec: authnv1.TokenRequestSpec{Audiences: []string{audience}, ExpirationSeconds: &expiry}}
	if pod != nil {
		req.Spec.BoundObjectRef = &authnv1.BoundObjectReference{Kind: "Pod", APIVersion: "v1", Name: pod.Name, UID: pod.UID}
	}
	got, err := clientset.CoreV1().ServiceAccounts(ns).CreateToken(context.Background(), sa, req, metav1.CreateOptions{})
	if err != nil {
		t.Fatal(err)
	}
	return got.Status.Token
}

// plainPod is a pod named agent in a namespace the runner didn't make, labelled like a lane's.
func plainPod(t *testing.T, ns string, nsLabels map[string]string, id string) *corev1.Pod {
	t.Helper()
	if err := k8s.Create(context.Background(), &corev1.Namespace{ObjectMeta: metav1.ObjectMeta{Name: ns, Labels: nsLabels}}); err != nil {
		t.Fatal(err)
	}
	ensureServiceAccount(t, ns)
	pod := &corev1.Pod{
		ObjectMeta: metav1.ObjectMeta{Name: "agent", Namespace: ns, Labels: map[string]string{LabelSession: id}},
		Spec:       corev1.PodSpec{ServiceAccountName: "default", Containers: []corev1.Container{{Name: "agent", Image: testImage}}},
	}
	if err := k8s.Create(context.Background(), pod); err != nil {
		t.Fatal(err)
	}
	return pod
}

func ask(header string) *httptest.ResponseRecorder {
	req := httptest.NewRequest(http.MethodGet, "/auth/git", nil)
	if header != "" {
		req.Header.Set("Authorization", header)
	}
	rec := httptest.NewRecorder()
	(&GitAuth{Client: mgr.GetClient(), Cache: mgr.GetCache(), Namespace: runnerNS}).ServeHTTP(rec, req)
	return rec
}

func basic(token string) string {
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	req.SetBasicAuth("x-access-token", token)
	return req.Header.Get("Authorization")
}

func TestGitAuth(t *testing.T) {
	demoRepo(t, "ac9")
	s := newSession(t, "ac9")
	ns := Namespace(s)
	ensureServiceAccount(t, ns)
	pod := &corev1.Pod{ObjectMeta: metav1.ObjectMeta{Name: "agent", Namespace: ns}}
	get(t, pod)
	valid := token(t, ns, pod, MirrorAudience)
	eventually(t, "a valid token is accepted", func() error {
		if rec := ask(basic(valid)); rec.Code != http.StatusOK {
			return fmt.Errorf("status %d", rec.Code)
		}
		return nil
	})

	other := plainPod(t, "plain-ac9", nil, s.Spec.ID)
	// A namespace that claims to be this session's lane, but isn't its namespace.
	fake := plainPod(t, "rl-fake-ac9", map[string]string{LabelLane: "true", LabelSession: s.Spec.ID}, s.Spec.ID)
	// Another pod in the lane, labelled like the agent: only the agent pod's token is the lane's identity.
	sibling := agentPod(s, options)
	sibling.Name = "sibling"
	if err := k8s.Create(context.Background(), sibling); err != nil {
		t.Fatal(err)
	}
	tests := []struct {
		name, header string
		want         int
	}{
		{"Basic password", basic(valid), http.StatusOK},
		{"Bearer", "Bearer " + valid, http.StatusOK},
		{"missing", "", http.StatusUnauthorized},
		{"empty Bearer", "Bearer ", http.StatusUnauthorized},
		{"not a token", "Bearer not-a-token", http.StatusUnauthorized},
		{"wrong audience", "Bearer " + token(t, ns, pod, "https://kubernetes.default.svc"), http.StatusUnauthorized},
		{"not bound to a pod", "Bearer " + token(t, ns, nil, MirrorAudience), http.StatusUnauthorized},
		{"bound to another pod in the lane", "Bearer " + token(t, ns, sibling, MirrorAudience), http.StatusUnauthorized},
		{"non-lane namespace", "Bearer " + token(t, other.Namespace, other, MirrorAudience), http.StatusUnauthorized},
		{"a namespace with the session label that isn't the lane", "Bearer " + token(t, fake.Namespace, fake, MirrorAudience), http.StatusUnauthorized},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			rec := ask(tt.header)
			if rec.Code != tt.want {
				t.Fatalf("status %d, want %d", rec.Code, tt.want)
			}
			want := ""
			if tt.want == http.StatusOK {
				want = "session-" + s.Spec.ID
			}
			if got := rec.Header().Get("X-Remote-User"); got != want {
				t.Errorf("X-Remote-User = %q, want %q", got, want)
			}
		})
	}
}

// The runner's own identity at the mirror: its ServiceAccount's token, bound to its pod.
func TestGitAuthRunner(t *testing.T) {
	ctx := context.Background()
	ensureServiceAccount(t, runnerNS) // the chart's ServiceAccount "runner" exists already
	runner := &corev1.Pod{
		ObjectMeta: metav1.ObjectMeta{Name: "runner-0", Namespace: runnerNS},
		Spec:       corev1.PodSpec{ServiceAccountName: "runner", Containers: []corev1.Container{{Name: "runner", Image: testImage}}},
	}
	if err := k8s.Create(ctx, runner); err != nil {
		t.Fatal(err)
	}
	// Another namespace's ServiceAccount called "runner" is nobody.
	other := plainPod(t, "runner-impostor", nil, "")
	if err := k8s.Create(ctx, &corev1.ServiceAccount{ObjectMeta: metav1.ObjectMeta{Name: "runner", Namespace: other.Namespace}}); err != nil {
		t.Fatal(err)
	}
	tests := []struct {
		name, header string
		want         int
	}{
		{"the runner's token", basic(saToken(t, runnerNS, "runner", runner, MirrorAudience)), http.StatusOK},
		{"another audience", "Bearer " + saToken(t, runnerNS, "runner", runner, "https://kubernetes.default.svc"), http.StatusUnauthorized},
		{"not bound to a pod", "Bearer " + saToken(t, runnerNS, "runner", nil, MirrorAudience), http.StatusUnauthorized},
		{"another ServiceAccount in the runner's namespace", "Bearer " + saToken(t, runnerNS, "default", nil, MirrorAudience), http.StatusUnauthorized},
		{"a runner ServiceAccount in another namespace", "Bearer " + saToken(t, other.Namespace, "runner", nil, MirrorAudience), http.StatusUnauthorized},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			rec := ask(tt.header)
			want := map[int]string{http.StatusOK: "runner"}[tt.want]
			if rec.Code != tt.want || rec.Header().Get("X-Remote-User") != want {
				t.Errorf("status %d, X-Remote-User %q; want %d, %q", rec.Code, rec.Header().Get("X-Remote-User"), tt.want, want)
			}
		})
	}
}

// The /auth/git listener bounds slow and oversized requests.
func TestServerLimits(t *testing.T) {
	srv := NewServer("127.0.0.1:0", &GitAuth{})
	if srv.ReadHeaderTimeout == 0 || srv.WriteTimeout == 0 || srv.IdleTimeout == 0 || srv.MaxHeaderBytes != 16<<10 {
		t.Fatalf("timeouts %v/%v/%v, max header %d", srv.ReadHeaderTimeout, srv.WriteTimeout, srv.IdleTimeout, srv.MaxHeaderBytes)
	}
	ts := httptest.NewUnstartedServer(srv.Handler)
	ts.Config = srv
	ts.Start()
	defer ts.Close()
	for size, want := range map[int]int{0: http.StatusOK, 64 << 10: http.StatusRequestHeaderFieldsTooLarge} {
		req, _ := http.NewRequest(http.MethodGet, ts.URL+"/healthz", nil)
		req.Header.Set("X-Pad", strings.Repeat("a", size))
		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatal(err)
		}
		_ = resp.Body.Close()
		if resp.StatusCode != want {
			t.Errorf("a %d-byte header: status %d, want %d", size, resp.StatusCode, want)
		}
	}
}
