package session

import (
	"bytes"
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/http/cgi" //nolint:gosec // a test stand-in for the mirror's nginx and fcgiwrap; httpoxy was fixed in Go 1.6.3
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/go-logr/logr"
	corev1 "k8s.io/api/core/v1"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/api/resource"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
	"k8s.io/apimachinery/pkg/runtime"
	utilyaml "k8s.io/apimachinery/pkg/util/yaml"
	"k8s.io/client-go/kubernetes"
	clientgoscheme "k8s.io/client-go/kubernetes/scheme"
	"k8s.io/client-go/rest"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/envtest"
	"sigs.k8s.io/controller-runtime/pkg/manager"
)

// The envtest suite: a real API server and etcd, the chart's Session CRD, a stand-in for Cilium's
// CRD, and a git mirror stand-in: the real `git http-backend` behind net/http/cgi. The chart's own
// RBAC and admission policy (rendered by helm) are applied, and the runner runs as its
// ServiceAccount, so every test also proves that those permissions are enough, and no more.
// setup-envtest (pinned by go.mod's tool directive) downloads the API server and etcd of
// envtestKubernetes, the release k8s.io/api is at, and verifies their checksums.

const (
	envtestKubernetes = "1.37.0"
	runnerNS          = "rangelight-runner"
	runnerUser        = "system:serviceaccount:" + runnerNS + ":runner"
	runnerToken       = "the-runner-test-token"
	chart             = "../../../../deployments/helm/rangelight"
	testImage         = "ghcr.io/rangelight/agent:test@sha256:1111111111111111111111111111111111111111111111111111111111111111"
)

var (
	k8s       client.Client // uncached, as an admin, for the tests' own reads and writes
	asRunner  client.Client // uncached, as the runner's ServiceAccount
	clientset *kubernetes.Clientset
	mgr       manager.Manager
	mirror    *testMirror
	options   Options
)

func TestMain(m *testing.M) {
	flag.Parse()
	// Fuzz targets are pure functions: fuzzing needs no API server.
	if f := flag.Lookup("test.fuzz"); f != nil && f.Value.String() != "" {
		os.Exit(m.Run())
	}
	log := slog.New(slog.NewJSONHandler(os.Stderr, &slog.HandlerOptions{Level: slog.LevelWarn}))
	slog.SetDefault(log)
	ctrl.SetLogger(logr.FromSlogHandler(log.Handler()))
	code, err := run(m)
	if err != nil {
		fmt.Fprintln(os.Stderr, "envtest:", err)
		code = 1
	}
	os.Exit(code)
}

func run(m *testing.M) (int, error) {
	// The fixture repos are made with the tests' own git settings, never the user's.
	for k, v := range map[string]string{"GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1"} {
		if err := os.Setenv(k, v); err != nil {
			return 1, err
		}
	}
	assets, err := exec.Command("go", "tool", "setup-envtest", "use", "-p", "path", envtestKubernetes).Output()
	if err != nil {
		return 1, fmt.Errorf("setup-envtest: %w", err)
	}
	env := &envtest.Environment{
		BinaryAssetsDirectory: string(assets),
		CRDDirectoryPaths:     []string{"../../../../deployments/helm/rangelight/crds", "testdata"},
		ErrorIfCRDPathMissing: true,
	}
	cfg, err := env.Start()
	if err != nil {
		return 1, fmt.Errorf("envtest: %w", err)
	}
	defer func() { _ = env.Stop() }()
	if mirror, err = newTestMirror(); err != nil {
		return 1, err
	}
	defer mirror.Close()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	if err := clients(ctx, cfg); err != nil {
		return 1, err
	}
	if err := applyChart(ctx); err != nil {
		return 1, err
	}
	if err := startRunner(ctx, cfg); err != nil {
		return 1, err
	}
	return m.Run(), nil
}

// clients makes the admin and runner clients, and the runner's namespace.
func clients(ctx context.Context, cfg *rest.Config) error {
	scheme := runtime.NewScheme()
	if err := errors.Join(clientgoscheme.AddToScheme(scheme), AddToScheme(scheme)); err != nil {
		return fmt.Errorf("scheme: %w", err)
	}
	var err error
	if k8s, err = client.New(cfg, client.Options{Scheme: scheme}); err != nil {
		return fmt.Errorf("client: %w", err)
	}
	if asRunner, err = client.New(runnerConfig(cfg), client.Options{Scheme: scheme}); err != nil {
		return fmt.Errorf("client: %w", err)
	}
	if clientset, err = kubernetes.NewForConfig(cfg); err != nil {
		return fmt.Errorf("clientset: %w", err)
	}
	if err := k8s.Create(ctx, &corev1.Namespace{ObjectMeta: metav1.ObjectMeta{Name: runnerNS}}); err != nil {
		return fmt.Errorf("namespace: %w", err)
	}
	return nil
}

// runnerConfig is cfg, acting as the runner's ServiceAccount.
func runnerConfig(cfg *rest.Config) *rest.Config {
	c := rest.CopyConfig(cfg)
	c.Impersonate = rest.ImpersonationConfig{
		UserName: runnerUser,
		Groups:   []string{"system:serviceaccounts", "system:serviceaccounts:" + runnerNS, "system:authenticated"},
	}
	return c
}

// applyChart applies the runner's RBAC and admission policy, as the chart renders them.
func applyChart(ctx context.Context) error {
	out, err := exec.Command("helm", "template", "rangelight", chart, "--namespace", "rangelight-system",
		"-f", chart+"/test/runner-values.yaml",
		"--show-only", "templates/runner/rbac.yaml", "--show-only", "templates/runner/admission.yaml").Output()
	if err != nil {
		return fmt.Errorf("helm template: %w", err)
	}
	dec := utilyaml.NewYAMLOrJSONDecoder(bytes.NewReader(out), 4096)
	for {
		var obj unstructured.Unstructured
		if err := dec.Decode(&obj.Object); errors.Is(err, io.EOF) {
			return nil
		} else if err != nil {
			return fmt.Errorf("chart: %w", err)
		}
		if len(obj.Object) == 0 {
			continue
		}
		if err := k8s.Create(ctx, &obj); err != nil {
			return fmt.Errorf("chart: %s %s: %w", obj.GetKind(), obj.GetName(), err)
		}
	}
}

func startRunner(ctx context.Context, cfg *rest.Config) error {
	token := filepath.Join(mirror.Root, "runner-token")
	if err := os.WriteFile(token, []byte(runnerToken+"\n"), 0o600); err != nil {
		return fmt.Errorf("token: %w", err)
	}
	options = Options{
		AgentImage: testImage, MirrorURL: mirror.URL, MirrorTokenFile: token, Workers: 4,
		Workspace: resource.MustParse("10Gi"), Tmp: resource.MustParse("1Gi"), EphemeralStorage: resource.MustParse("12Gi"),
	}
	var err error
	if mgr, err = NewManager(runnerConfig(cfg), runnerNS); err != nil {
		return err
	}
	r := &Reconciler{Client: mgr.GetClient(), Reader: mgr.GetAPIReader(), Options: options}
	if err := r.SetupWithManager(mgr); err != nil {
		return err
	}
	go func() { _ = mgr.Start(ctx) }()
	if !mgr.GetCache().WaitForCacheSync(ctx) {
		return fmt.Errorf("cache did not sync")
	}
	return nil
}

// testMirror serves bare repos under Root the way the git mirror does, at /<repo>.git. It records
// each repo's last Authorization header, and holds every request for slow.git until the client
// gives up (or the mirror closes).
type testMirror struct {
	*httptest.Server
	Root string
	mu   sync.Mutex
	auth map[string]string
	slow chan struct{}
}

func newTestMirror() (*testMirror, error) {
	root, err := os.MkdirTemp("", "rl-mirror-")
	if err != nil {
		return nil, fmt.Errorf("mirror: %w", err)
	}
	gitPath, err := exec.LookPath("git")
	if err != nil {
		return nil, fmt.Errorf("mirror: %w", err)
	}
	backend := &cgi.Handler{Path: gitPath, Args: []string{"http-backend"}, Env: []string{"GIT_PROJECT_ROOT=" + root, "GIT_HTTP_EXPORT_ALL=1"}}
	m := &testMirror{Root: root, auth: map[string]string{}, slow: make(chan struct{})}
	m.Server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		repo, _, _ := strings.Cut(strings.TrimPrefix(r.URL.Path, "/"), "/")
		m.mu.Lock()
		m.auth[repo] = r.Header.Get("Authorization")
		m.mu.Unlock()
		if repo == "slow.git" {
			select {
			case <-m.slow:
			case <-r.Context().Done():
			}
			return
		}
		backend.ServeHTTP(w, r)
	}))
	return m, nil
}

// authorization is the Authorization header of the last request for <repo>.git.
func (m *testMirror) authorization(repo string) string {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.auth[repo+".git"]
}

// Close stops the server and removes the repos.
func (m *testMirror) Close() {
	close(m.slow)
	m.Server.Close()
	_ = os.RemoveAll(m.Root)
}

// repo creates <name>.git whose branches each hold one commit with the given files.
func (m *testMirror) repo(t *testing.T, name string, branches map[string]map[string]string) map[string]string {
	t.Helper()
	bare := filepath.Join(m.Root, name+".git")
	work := t.TempDir()
	runGit(t, "", "init", "-q", "--bare", "-b", "main", bare)
	runGit(t, "", "init", "-q", "-b", "main", work)
	heads := map[string]string{}
	for branch, files := range branches {
		runGit(t, work, "checkout", "-q", "--orphan", branch)
		runGit(t, work, "rm", "-rfq", "--ignore-unmatch", ".")
		for path, content := range files {
			if err := os.WriteFile(filepath.Join(work, path), []byte(content), 0o600); err != nil {
				t.Fatal(err)
			}
		}
		runGit(t, work, "add", "-A")
		runGit(t, work, "-c", "user.name=test", "-c", "user.email=test@example.com", "commit", "-q", "--allow-empty", "-m", branch)
		runGit(t, work, "push", "-q", bare, "HEAD:refs/heads/"+branch)
		heads[branch] = runGit(t, work, "rev-parse", "HEAD")
	}
	return heads
}

func runGit(t *testing.T, dir string, args ...string) string {
	t.Helper()
	cmd := exec.Command("git", args...) //nolint:gosec // the tests' own git calls
	cmd.Dir = dir
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("git %v: %v\n%s", args, err, out)
	}
	return string(out[:max(0, len(out)-1)])
}

// eventually polls f until it returns nil, or fails the test with its last error.
func eventually(t *testing.T, what string, f func() error) {
	t.Helper()
	deadline := time.Now().Add(30 * time.Second)
	for {
		err := f()
		if err == nil {
			return
		}
		if time.Now().After(deadline) {
			t.Fatalf("%s: %v", what, err)
		}
		time.Sleep(100 * time.Millisecond)
	}
}

// ensureServiceAccount creates the namespace's default ServiceAccount, which the cluster's own
// controller would create; envtest runs none.
func ensureServiceAccount(t *testing.T, ns string) {
	t.Helper()
	eventually(t, "service account in "+ns, func() error {
		sa := &corev1.ServiceAccount{ObjectMeta: metav1.ObjectMeta{Name: "default", Namespace: ns}}
		if err := k8s.Create(context.Background(), sa); err != nil && !apierrors.IsAlreadyExists(err) {
			return err
		}
		return nil
	})
}
