package session

import (
	"bytes"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

// The committed DeepCopy code is what controller-gen (pinned by go.mod's tool directive) writes.
func TestDeepCopyIsGenerated(t *testing.T) {
	dir := t.TempDir()
	out, err := exec.Command("go", "tool", "controller-gen", "object", "paths=.", "output:object:dir="+dir).CombinedOutput() //nolint:gosec // a pinned tool, a temp dir
	if err != nil {
		t.Fatalf("controller-gen: %v\n%s", err, out)
	}
	want, err := os.ReadFile(filepath.Join(dir, "zz_generated.deepcopy.go")) //nolint:gosec // the test's temp dir
	if err != nil {
		t.Fatal(err)
	}
	if got, err := os.ReadFile("zz_generated.deepcopy.go"); err != nil || !bytes.Equal(got, want) {
		t.Errorf("zz_generated.deepcopy.go is stale (%v); run in this directory: go tool controller-gen object paths=.", err)
	}
}
