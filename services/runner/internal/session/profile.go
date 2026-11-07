package session

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"
	"time"

	"github.com/go-git/go-billy/v5/osfs"
	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/cache"
	"github.com/go-git/go-git/v5/plumbing/object"
	githttp "github.com/go-git/go-git/v5/plumbing/transport/http"
	"github.com/go-git/go-git/v5/storage/filesystem"
	"github.com/santhosh-tekuri/jsonschema/v6"
	"sigs.k8s.io/yaml"

	"github.com/rangelight/rangelight/services/runner/internal/session/schema"
)

const (
	profileFile    = "rangelight.yaml"
	profileMaxSize = 256 << 10 // the ConfigMap also holds the ticket, and is capped at 1 MiB
	// One fetch holds one worker (Options.Workers) for at most this long.
	fetchTimeout = 30 * time.Second
)

// failedError is a problem with the session itself, not with the cluster or the network: retrying
// won't fix it, so the session goes Failed with this message.
type failedError struct{ msg string }

func (e *failedError) Error() string { return e.msg }

func failed(format string, args ...any) error { return &failedError{msg: fmt.Sprintf(format, args...)} }

// profiles reads session profiles from the git mirror, as the runner (read-only there, RAN-17).
type profiles struct {
	mirrorURL, tokenFile string
	schema               *jsonschema.Schema
}

func newProfiles(mirrorURL, tokenFile string) (*profiles, error) {
	doc, err := jsonschema.UnmarshalJSON(bytes.NewReader(schema.ProfileV1alpha1))
	if err != nil {
		return nil, fmt.Errorf("profile schema: %w", err)
	}
	const url = "https://rangelight.dev/schemas/rangelight.v1alpha1.schema.json"
	c := jsonschema.NewCompiler()
	if err := c.AddResource(url, doc); err != nil {
		return nil, fmt.Errorf("profile schema: %w", err)
	}
	sch, err := c.Compile(url)
	if err != nil {
		return nil, fmt.Errorf("profile schema: %w", err)
	}
	return &profiles{mirrorURL: mirrorURL, tokenFile: tokenFile, schema: sch}, nil
}

// read reads rangelight.yaml at the tip of baseRef in the mirror's <repo>.git, never from the
// session's branches (ADR 0041), and returns the commit and the profile as JSON. A missing or
// invalid profile is a failedError; a fetch that fails is retried.
//
// ponytail: a depth-1 clone into a temporary directory; the ceiling is one base-branch snapshot on
// the runner's /tmp per worker at once (size the chart's runner.tmpSize to Workers of them).
// Upgrade: a partial clone (blob:none) once the mirror sets uploadpack.allowFilter.
func (p *profiles) read(ctx context.Context, repo, baseRef string) (string, []byte, error) {
	// The projected token rotates: read it at each fetch. It goes only into the request's
	// Authorization header, never into the URL, argv or a log line.
	token, err := os.ReadFile(p.tokenFile)
	if err != nil {
		return "", nil, fmt.Errorf("profile: mirror token: %w", err)
	}
	dir, err := os.MkdirTemp("", "rl-profile-")
	if err != nil {
		return "", nil, fmt.Errorf("profile: %w", err)
	}
	defer func() { _ = os.RemoveAll(dir) }()
	ctx, cancel := context.WithTimeout(ctx, fetchTimeout)
	defer cancel()
	store := filesystem.NewStorage(osfs.New(dir), cache.NewObjectLRUDefault())
	r, err := git.CloneContext(ctx, store, nil, &git.CloneOptions{
		URL:           p.mirrorURL + "/" + repo + ".git",
		ReferenceName: plumbing.NewBranchReferenceName(baseRef),
		SingleBranch:  true,
		Depth:         1,
		NoCheckout:    true,
		Tags:          git.NoTags,
		Auth:          &githttp.BasicAuth{Username: "runner", Password: strings.TrimSpace(string(token))},
	})
	if err != nil {
		return "", nil, fmt.Errorf("profile: fetch %s at %s: %w", repo, baseRef, err)
	}
	head, err := r.Head()
	if err != nil {
		return "", nil, fmt.Errorf("profile: %w", err)
	}
	commit, err := r.CommitObject(head.Hash())
	if err != nil {
		return "", nil, fmt.Errorf("profile: %w", err)
	}
	profile, err := p.at(commit)
	return head.Hash().String(), profile, err
}

func (p *profiles) at(commit *object.Commit) ([]byte, error) {
	f, err := commit.File(profileFile)
	if errors.Is(err, object.ErrFileNotFound) {
		return nil, failed("%s: not found at %s", profileFile, commit.Hash)
	}
	if err != nil {
		return nil, fmt.Errorf("profile: %w", err)
	}
	if f.Size > profileMaxSize {
		return nil, failed("%s: %d bytes, more than %d", profileFile, f.Size, profileMaxSize)
	}
	rd, err := f.Reader()
	if err != nil {
		return nil, fmt.Errorf("profile: %w", err)
	}
	defer func() { _ = rd.Close() }()
	raw, err := io.ReadAll(rd)
	if err != nil {
		return nil, fmt.Errorf("profile: %w", err)
	}
	return p.valid(raw)
}

// valid turns rangelight.yaml into JSON, valid against the embedded v1alpha1 schema.
func (p *profiles) valid(raw []byte) ([]byte, error) {
	js, err := yaml.YAMLToJSONStrict(raw)
	if err != nil {
		return nil, failed("%s: %v", profileFile, err)
	}
	doc, err := jsonschema.UnmarshalJSON(bytes.NewReader(js))
	if err != nil {
		return nil, failed("%s: %v", profileFile, err)
	}
	if err := p.schema.Validate(doc); err != nil {
		return nil, failed("%s: %v", profileFile, err)
	}
	var out bytes.Buffer
	if err := json.Compact(&out, js); err != nil {
		return nil, failed("%s: %v", profileFile, err)
	}
	return out.Bytes(), nil
}
