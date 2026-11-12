// Package github is the runner's GitHub App client (ADR 0011, ADR 0040 §3): an installation token
// for one repository, and the pull request of a branch. The App's id and private key are files,
// read at each use; a token never reaches a URL, a file or a log.
package github

import (
	"bytes"
	"context"
	"crypto"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"
)

// timeout bounds each GitHub API call.
const timeout = 15 * time.Second

// App is a GitHub App.
type App struct {
	// API is the GitHub API's base URL, without a trailing slash (https://api.github.com).
	API string
	// IDFile and KeyFile hold the App's id and its PEM private key.
	IDFile, KeyFile string
}

// PullRequest is the pull request to open from Head into Base of Owner/Repo.
type PullRequest struct {
	Owner, Repo, Head, Base, Title, Body string
}

// Token is an installation token for owner/repo only, with contents:write and pull_requests:write,
// valid for an hour.
func (a *App) Token(ctx context.Context, owner, repo string) (string, error) {
	jwt, err := a.jwt(time.Now())
	if err != nil {
		return "", err
	}
	var inst struct {
		ID int64 `json:"id"`
	}
	if err := a.call(ctx, http.MethodGet, "/repos/"+path(owner, repo)+"/installation", "Bearer "+jwt, nil, &inst); err != nil {
		return "", fmt.Errorf("installation of %s/%s: %w", owner, repo, err)
	}
	req := map[string]any{
		"repositories": []string{repo},
		"permissions":  map[string]string{"contents": "write", "pull_requests": "write"},
	}
	var tok struct {
		Token string `json:"token"`
	}
	err = a.call(ctx, http.MethodPost, "/app/installations/"+strconv.FormatInt(inst.ID, 10)+"/access_tokens", "Bearer "+jwt, req, &tok)
	if err != nil {
		return "", fmt.Errorf("installation token for %s/%s: %w", owner, repo, err)
	}
	if tok.Token == "" {
		return "", fmt.Errorf("installation token for %s/%s: empty", owner, repo)
	}
	return tok.Token, nil
}

// Open is the URL of the open pull request from pr.Head, opened first if there is none, so a
// branch has one open pull request however often it is published.
func (a *App) Open(ctx context.Context, token string, pr PullRequest) (string, error) {
	type found struct {
		URL string `json:"html_url"`
	}
	var open []found
	q := url.Values{"head": {pr.Owner + ":" + pr.Head}, "state": {"open"}}
	if err := a.call(ctx, http.MethodGet, "/repos/"+path(pr.Owner, pr.Repo)+"/pulls?"+q.Encode(), "Bearer "+token, nil, &open); err != nil {
		return "", fmt.Errorf("pull requests of %s: %w", pr.Head, err)
	}
	if len(open) > 0 {
		return open[0].URL, nil
	}
	var made found
	body := map[string]string{"title": pr.Title, "head": pr.Head, "base": pr.Base, "body": pr.Body}
	if err := a.call(ctx, http.MethodPost, "/repos/"+path(pr.Owner, pr.Repo)+"/pulls", "Bearer "+token, body, &made); err != nil {
		return "", fmt.Errorf("open a pull request for %s: %w", pr.Head, err)
	}
	return made.URL, nil
}

// Revoke ends an installation token before its hour is up.
func (a *App) Revoke(ctx context.Context, token string) error {
	if err := a.call(ctx, http.MethodDelete, "/installation/token", "Bearer "+token, nil, nil); err != nil {
		return fmt.Errorf("revoke the installation token: %w", err)
	}
	return nil
}

func path(owner, repo string) string { return url.PathEscape(owner) + "/" + url.PathEscape(repo) }

// call sends one API request and decodes the 2xx answer into out, unless out is nil. The error carries the status
// and GitHub's message, never the request's headers.
func (a *App) call(ctx context.Context, method, path, auth string, in, out any) error {
	var body io.Reader
	if in != nil {
		raw, err := json.Marshal(in)
		if err != nil {
			return fmt.Errorf("request: %w", err)
		}
		body = bytes.NewReader(raw)
	}
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, method, a.API+path, body)
	if err != nil {
		return fmt.Errorf("request: %w", err)
	}
	req.Header.Set("Authorization", auth)
	req.Header.Set("Accept", "application/vnd.github+json")
	req.Header.Set("X-GitHub-Api-Version", "2022-11-28")
	if in != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return fmt.Errorf("GitHub: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return fmt.Errorf("GitHub's answer: %w", err)
	}
	if resp.StatusCode/100 != 2 {
		var e struct {
			Message string `json:"message"`
		}
		_ = json.Unmarshal(raw, &e)
		return fmt.Errorf("GitHub answered %d: %s", resp.StatusCode, e.Message)
	}
	if out == nil {
		return nil
	}
	if err := json.Unmarshal(raw, out); err != nil {
		return fmt.Errorf("GitHub's answer: %w", err)
	}
	return nil
}

// jwt is the App's own token: RS256, issued a minute back and expiring in 9 minutes, so a clock up to
// a minute off either way stays inside GitHub's 10-minute limit.
func (a *App) jwt(now time.Time) (string, error) {
	id, err := os.ReadFile(a.IDFile)
	if err != nil {
		return "", fmt.Errorf("GitHub App id: %w", err)
	}
	pemKey, err := os.ReadFile(a.KeyFile)
	if err != nil {
		return "", fmt.Errorf("GitHub App private key: %w", err)
	}
	key, err := privateKey(pemKey)
	if err != nil {
		return "", fmt.Errorf("GitHub App private key: %w", err)
	}
	claims, err := json.Marshal(map[string]any{
		"iss": strings.TrimSpace(string(id)), "iat": now.Add(-time.Minute).Unix(), "exp": now.Add(9 * time.Minute).Unix(),
	})
	if err != nil {
		return "", fmt.Errorf("GitHub App token: %w", err)
	}
	enc := base64.RawURLEncoding
	signed := enc.EncodeToString([]byte(`{"alg":"RS256","typ":"JWT"}`)) + "." + enc.EncodeToString(claims)
	sum := sha256.Sum256([]byte(signed))
	sig, err := rsa.SignPKCS1v15(rand.Reader, key, crypto.SHA256, sum[:])
	if err != nil {
		return "", fmt.Errorf("GitHub App token: %w", err)
	}
	return signed + "." + enc.EncodeToString(sig), nil
}

// privateKey reads GitHub's PKCS#1 key, or a PKCS#8 one.
func privateKey(raw []byte) (*rsa.PrivateKey, error) {
	block, _ := pem.Decode(raw)
	if block == nil {
		return nil, errors.New("not PEM")
	}
	if k, err := x509.ParsePKCS1PrivateKey(block.Bytes); err == nil {
		return k, nil
	}
	k, err := x509.ParsePKCS8PrivateKey(block.Bytes)
	if err != nil {
		return nil, errors.New("not an RSA private key")
	}
	rk, ok := k.(*rsa.PrivateKey)
	if !ok {
		return nil, errors.New("not an RSA private key")
	}
	return rk, nil
}
