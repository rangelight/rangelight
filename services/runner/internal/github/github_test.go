package github

import (
	"context"
	"crypto"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// app is an App whose id and key are files in a test directory; pkcs8 picks the key's encoding.
func app(t *testing.T, api string, pkcs8 bool) (*App, *rsa.PublicKey) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	block := &pem.Block{Type: "RSA PRIVATE KEY", Bytes: x509.MarshalPKCS1PrivateKey(key)}
	if pkcs8 {
		der, err := x509.MarshalPKCS8PrivateKey(key)
		if err != nil {
			t.Fatal(err)
		}
		block = &pem.Block{Type: "PRIVATE KEY", Bytes: der}
	}
	dir := t.TempDir()
	a := &App{API: api, IDFile: filepath.Join(dir, "app-id"), KeyFile: filepath.Join(dir, "private-key")}
	if err := os.WriteFile(a.IDFile, []byte("12345\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(a.KeyFile, pem.EncodeToMemory(block), 0o600); err != nil {
		t.Fatal(err)
	}
	return a, &key.PublicKey
}

func TestJWTIsRS256ForNineMinutes(t *testing.T) {
	for _, pkcs8 := range []bool{false, true} {
		a, pub := app(t, "", pkcs8)
		now := time.Unix(1_800_000_000, 0)
		jwt, err := a.jwt(now)
		if err != nil {
			t.Fatal(err)
		}
		parts := strings.Split(jwt, ".")
		if len(parts) != 3 {
			t.Fatalf("not a JWT: %q", jwt)
		}
		sig, err := base64.RawURLEncoding.DecodeString(parts[2])
		if err != nil {
			t.Fatal(err)
		}
		sum := sha256.Sum256([]byte(parts[0] + "." + parts[1]))
		if err := rsa.VerifyPKCS1v15(pub, crypto.SHA256, sum[:], sig); err != nil {
			t.Fatalf("signature (pkcs8 %v): %v", pkcs8, err)
		}
		header, _ := base64.RawURLEncoding.DecodeString(parts[0])
		raw, _ := base64.RawURLEncoding.DecodeString(parts[1])
		var claims struct {
			Iss      string
			Iat, Exp int64
		}
		if err := json.Unmarshal(raw, &claims); err != nil {
			t.Fatal(err)
		}
		if string(header) != `{"alg":"RS256","typ":"JWT"}` || claims.Iss != "12345" ||
			claims.Iat != now.Unix()-60 || claims.Exp != now.Unix()+540 {
			t.Errorf("header %s, claims %+v", header, claims)
		}
	}
}

func TestNotAKey(t *testing.T) {
	a, _ := app(t, "", false)
	if err := os.WriteFile(a.KeyFile, []byte("not a key"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := a.Token(context.Background(), "o", "r"); err == nil || !strings.Contains(err.Error(), "not PEM") {
		t.Fatalf("error %v", err)
	}
}

// After a restart the runner knows no pull request: Open finds the open one by its head branch
// and never opens a second.
func TestOpenFindsTheOpenPullRequest(t *testing.T) {
	var created int
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.Header.Get("Authorization") != "Bearer ghs_x":
			w.WriteHeader(http.StatusUnauthorized)
		case r.Method == http.MethodGet && r.URL.Path == "/repos/o/r/pulls" &&
			r.URL.Query().Get("head") == "o:rangelight/1/work" && r.URL.Query().Get("state") == "open":
			_, _ = w.Write([]byte(`[{"html_url":"https://github.com/o/r/pull/7"}]`))
		case r.Method == http.MethodGet && r.URL.Path == "/repos/o/r/pulls":
			_, _ = w.Write([]byte(`[]`))
		case r.Method == http.MethodDelete && r.URL.Path == "/installation/token":
			w.WriteHeader(http.StatusNoContent)
		case r.Method == http.MethodPost && r.URL.Path == "/repos/o/r/pulls":
			created++
			w.WriteHeader(http.StatusCreated)
			_, _ = w.Write([]byte(`{"html_url":"https://github.com/o/r/pull/8"}`))
		default:
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer srv.Close()
	a := &App{API: srv.URL}
	for head, want := range map[string]string{"rangelight/1/work": "https://github.com/o/r/pull/7", "rangelight/2/work": "https://github.com/o/r/pull/8"} {
		got, err := a.Open(context.Background(), "ghs_x", PullRequest{Owner: "o", Repo: "r", Head: head, Base: "main"})
		if err != nil || got != want {
			t.Errorf("%s: %q, %v; want %q", head, got, err, want)
		}
	}
	if created != 1 {
		t.Errorf("opened %d pull requests, want 1", created)
	}
	if err := a.Revoke(context.Background(), "ghs_x"); err != nil {
		t.Errorf("revoke: %v", err)
	}
	if _, err := a.Open(context.Background(), "wrong", PullRequest{Owner: "o", Repo: "r", Head: "h"}); err == nil ||
		!strings.Contains(err.Error(), "GitHub answered 401") || strings.Contains(err.Error(), "wrong") {
		t.Errorf("error %v", err)
	}
}
