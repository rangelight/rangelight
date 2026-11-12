package session

import (
	"bytes"
	"context"
	"encoding/base64"
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
)

// RAN-65 AC1: without a GitHub App, a pushed session says publishing is off; with one, it doesn't.
func TestPublishingOffSaysSo(t *testing.T) {
	off := &Reconciler{}
	s := &Session{Status: Status{Pushes: 1}}
	if wait := off.publishDue(context.Background(), s); wait != 0 || s.Status.PublishMessage != publishingOff {
		t.Errorf("wait %v, message %q", wait, s.Status.PublishMessage)
	}
	idle := &Session{}
	if off.publishDue(context.Background(), idle); idle.Status.PublishMessage != "" {
		t.Errorf("a session that never pushed: %q", idle.Status.PublishMessage)
	}
}

// RAN-65 AC2: deleting a Session removes its repo from the git mirror, as the runner.
func TestDeleteRemovesTheMirrorRepo(t *testing.T) {
	demoRepo(t, "closed")
	s := newSession(t, "closed")
	pushed(t, s, laneToken(t, s), "work")
	repo := filepath.Join(mirror.Root, "sessions", s.Spec.ID+".git")
	if _, err := os.Stat(repo); err != nil {
		t.Fatal(err)
	}
	if err := k8s.Delete(context.Background(), s); err != nil {
		t.Fatal(err)
	}
	gone(t, s)
	mirror.mu.Lock()
	deleted, auth := slices.Clone(mirror.deleted), mirror.auth["sessions"]
	mirror.mu.Unlock()
	if !slices.Contains(deleted, "/sessions/"+s.Spec.ID+".git") {
		t.Errorf("the mirror got DELETE %v", deleted)
	}
	if want := "Basic " + base64.StdEncoding.EncodeToString([]byte("runner:"+runnerToken)); auth != want {
		t.Errorf("the DELETE's credentials %q, want the runner's", auth)
	}
	if _, err := os.Stat(repo); !os.IsNotExist(err) {
		t.Errorf("the session's mirror repo is still there: %v", err)
	}
}

// deleting is a Session the runner holds with its finalizer, deleted while the mirror's DELETEs
// fail, as it stands then.
func deleting(t *testing.T) *Session {
	t.Helper()
	s := newSession(t, "closed-down")
	eventually(t, "the runner's finalizer", func() error {
		if !slices.Contains(reload(t, s).Finalizers, finalizer) {
			return errors.New("no finalizer yet")
		}
		return nil
	})
	mirror.setDown(true)
	t.Cleanup(func() { mirror.setDown(false) })
	if err := k8s.Delete(context.Background(), s); err != nil {
		t.Fatal(err)
	}
	return reload(t, s)
}

// RAN-65 review F1: without a mirror there is nothing to remove, so a deleted Session goes at once.
func TestDeleteWithoutAMirrorIsAtOnce(t *testing.T) {
	s := deleting(t)
	noMirror := *theRunner
	noMirror.MirrorURL = ""
	if err := noMirror.remove(context.Background(), s); err != nil {
		t.Fatal(err)
	}
	gone(t, s)
}

// RAN-65 review F1: a mirror that stays down holds a deleted Session only until the bound, then the
// runner says which repo it leaves behind, and lets the Session go.
func TestDeleteGivesUpOnADownMirrorAfterTheBound(t *testing.T) {
	s := deleting(t)
	if err := theRunner.remove(context.Background(), s); err == nil {
		t.Fatal("a down mirror let the Session go before the bound")
	}
	logs := &syncBuffer{}
	defer slog.SetDefault(slog.Default())
	slog.SetDefault(slog.New(slog.NewTextHandler(logs, nil)))
	bounded := *theRunner
	bounded.mirrorGiveUp = 0 // the bound has passed
	if err := bounded.remove(context.Background(), reload(t, s)); err != nil {
		t.Fatal(err)
	}
	gone(t, s)
	if got := logs.String(); strings.Count(got, "giving up") != 1 || !strings.Contains(got, "/srv/git/sessions/"+s.Spec.ID+".git") {
		t.Errorf("want one warning naming the repo left behind: %s", got)
	}
}

type syncBuffer struct {
	mu sync.Mutex
	b  bytes.Buffer
}

func (b *syncBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.b.Write(p)
}

func (b *syncBuffer) String() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.b.String()
}

// Without a git mirror a lane can't read its profile: the session fails, saying why.
func TestNoMirrorFailsTheLane(t *testing.T) {
	_, _, err := (&profiles{}).read(context.Background(), "demo", "main")
	var f *failedError
	if !errors.As(err, &f) || !strings.Contains(f.msg, "no git mirror is deployed") {
		t.Errorf("error %v", err)
	}
}
