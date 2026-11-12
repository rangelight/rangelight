package session

import (
	"bytes"
	"crypto"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"

	"github.com/rangelight/rangelight/services/runner/internal/github"
)

// The repo the publish tests work on: published.git on the mirror, whose upstream is a bare repo
// on disk (file://), owned by "acme" at the fake GitHub.
const (
	publishedRepo = "published"
	ghToken       = "ghs_installation-token" //nolint:gosec // the fake GitHub's token
)

// testGitHub is a fake GitHub API: it checks the App's JWT, mints one installation token, and
// keeps the open pull requests by head branch.
type testGitHub struct {
	*httptest.Server
	key         *rsa.PublicKey
	mu          sync.Mutex
	failTokens  bool
	tokenAsks   []string          // each access-token request's body
	pulls       map[string]string // open pull request URL by head branch
	pullBodies  []map[string]string
	pullsOpened int
	revoked     int
}

// newTestGitHub starts the fake, and returns the App the runner uses with it.
func newTestGitHub(dir string) (*testGitHub, *github.App, error) {
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		return nil, nil, err
	}
	app := &github.App{IDFile: filepath.Join(dir, "app-id"), KeyFile: filepath.Join(dir, "private-key")}
	if err := os.WriteFile(app.IDFile, []byte("4242\n"), 0o600); err != nil {
		return nil, nil, err
	}
	pemKey := pem.EncodeToMemory(&pem.Block{Type: "RSA PRIVATE KEY", Bytes: x509.MarshalPKCS1PrivateKey(key)})
	if err := os.WriteFile(app.KeyFile, pemKey, 0o600); err != nil {
		return nil, nil, err
	}
	g := &testGitHub{key: &key.PublicKey, pulls: map[string]string{}}
	g.Server = httptest.NewServer(http.HandlerFunc(g.serve))
	app.API = g.URL
	return g, app, nil
}

func (g *testGitHub) serve(w http.ResponseWriter, r *http.Request) {
	g.mu.Lock()
	defer g.mu.Unlock()
	auth := r.Header.Get("Authorization")
	answer := func(code int, v any) {
		w.WriteHeader(code)
		_ = json.NewEncoder(w).Encode(v)
	}
	switch {
	case r.Method == http.MethodGet && r.URL.Path == "/repos/acme/published/installation" && g.appJWT(auth):
		answer(http.StatusOK, map[string]any{"id": 7})
	case r.Method == http.MethodPost && r.URL.Path == "/app/installations/7/access_tokens" && g.appJWT(auth):
		var body bytes.Buffer
		_, _ = body.ReadFrom(r.Body)
		g.tokenAsks = append(g.tokenAsks, body.String())
		if g.failTokens {
			answer(http.StatusInternalServerError, map[string]string{"message": "token service down"})
			return
		}
		answer(http.StatusCreated, map[string]string{"token": ghToken})
	case r.Method == http.MethodDelete && r.URL.Path == "/installation/token" && auth == "Bearer "+ghToken:
		g.revoked++
		w.WriteHeader(http.StatusNoContent)
	case r.URL.Path == "/repos/acme/published/pulls" && auth == "Bearer "+ghToken && r.Method == http.MethodGet:
		head, _ := strings.CutPrefix(r.URL.Query().Get("head"), "acme:")
		open := []map[string]string{}
		if u, ok := g.pulls[head]; ok && r.URL.Query().Get("state") == "open" {
			open = append(open, map[string]string{"html_url": u})
		}
		answer(http.StatusOK, open)
	case r.URL.Path == "/repos/acme/published/pulls" && auth == "Bearer "+ghToken && r.Method == http.MethodPost:
		var pr map[string]string
		_ = json.NewDecoder(r.Body).Decode(&pr)
		g.pullsOpened++
		g.pulls[pr["head"]] = fmt.Sprintf("https://github.com/acme/published/pull/%d", g.pullsOpened)
		g.pullBodies = append(g.pullBodies, pr)
		answer(http.StatusCreated, map[string]string{"html_url": g.pulls[pr["head"]]})
	default:
		answer(http.StatusUnauthorized, map[string]string{"message": "Bad credentials"})
	}
}

// appJWT checks "Bearer <the App's RS256 JWT>", issued by App 4242.
func (g *testGitHub) appJWT(auth string) bool {
	jwt, ok := strings.CutPrefix(auth, "Bearer ")
	parts := strings.Split(jwt, ".")
	if !ok || len(parts) != 3 {
		return false
	}
	sig, err := base64.RawURLEncoding.DecodeString(parts[2])
	sum := sha256.Sum256([]byte(parts[0] + "." + parts[1]))
	if err != nil || rsa.VerifyPKCS1v15(g.key, crypto.SHA256, sum[:], sig) != nil {
		return false
	}
	raw, _ := base64.RawURLEncoding.DecodeString(parts[1])
	var claims struct{ Iss string }
	return json.Unmarshal(raw, &claims) == nil && claims.Iss == "4242"
}

func (g *testGitHub) state() (opened, revoked int, asks []string, bodies []map[string]string) {
	g.mu.Lock()
	defer g.mu.Unlock()
	return g.pullsOpened, g.revoked, append([]string(nil), g.tokenAsks...), append([]map[string]string(nil), g.pullBodies...)
}

func (g *testGitHub) setFailTokens(fail bool) {
	g.mu.Lock()
	defer g.mu.Unlock()
	g.failTokens = fail
}
