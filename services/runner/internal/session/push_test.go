package session

import (
	"context"
	"fmt"
	"net/http"
	"sync"
	"testing"

	batchv1 "k8s.io/api/batch/v1"
	corev1 "k8s.io/api/core/v1"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"
)

// Two push reports at once both count: the counter goes up by exactly two.
func TestConcurrentPushReportsBothCount(t *testing.T) {
	s := newSession(t, publishedRepo)
	own := laneToken(t, s)
	eventually(t, "the lane's token is accepted", func() error {
		if rec := ask("Bearer " + own); rec.Code != http.StatusOK {
			return fmt.Errorf("status %d", rec.Code)
		}
		return nil
	})
	before := reload(t, s).Status.Pushes
	gh.setFailTokens(true) // no Job gets released meanwhile: only the count matters here
	defer gh.setFailTokens(false)
	var wg sync.WaitGroup
	codes := make([]int, 2)
	for i := range codes {
		wg.Go(func() {
			codes[i] = reportPush("Bearer "+own, s.Spec.ID, "refs/heads/"+workBranch(s.Spec.ID), fmt.Sprintf("%040d", i+1))
		})
	}
	wg.Wait()
	if got := reload(t, s).Status.Pushes - before; codes[0] != http.StatusAccepted || codes[1] != http.StatusAccepted || got != 2 {
		t.Errorf("statuses %v, pushes went up by %d, want 202s and 2", codes, got)
	}
}

// RAN-55 review MEDIUM: a Job that never started (its token failed) is superseded by the next push,
// which publishes; no Job of the older push is left behind.
func TestANewerPushSupersedesAJobThatNeverStarted(t *testing.T) {
	s := newSession(t, publishedRepo)
	own := laneToken(t, s)
	gh.setFailTokens(true)
	pushed(t, s, own, "never-started")
	older := fmt.Sprintf("publish-%s-%d", s.Spec.ID, reload(t, s).Status.Pushes)
	publishMessage(t, s, "token service down")
	newer := pushed(t, s, own, "newer")
	gh.setFailTokens(false)
	runJob(t, releasedJob(t, s))
	published(t, s, newer)
	var job batchv1.Job
	if err := k8s.Get(context.Background(), client.ObjectKey{Namespace: runnerNS, Name: older}, &job); !apierrors.IsNotFound(err) {
		t.Errorf("the superseded Job %s is still there: %v", older, err)
	}
}

// A superseded Job that has its token Secret (the runner stopped between the Secret and the release)
// has that token revoked when it is removed.
func TestASupersededJobsTokenIsRevoked(t *testing.T) {
	ctx := context.Background()
	s := newSession(t, publishedRepo)
	stale := s.DeepCopy()
	stale.Status.Pushes = 1
	job := newPublishJob(stale, options, "file://"+upstreamDir())
	if err := k8s.Create(ctx, job); err != nil {
		t.Fatal(err)
	}
	secret := &corev1.Secret{ObjectMeta: metav1.ObjectMeta{Name: job.Name, Namespace: runnerNS}, Data: map[string][]byte{"token": []byte(ghToken)}}
	if err := k8s.Create(ctx, secret); err != nil {
		t.Fatal(err)
	}
	eventually(t, "the runner sees the Job", func() error {
		return mgr.GetCache().Get(ctx, client.ObjectKeyFromObject(job), &batchv1.Job{})
	})
	_, revoked, _, _ := gh.state()
	busy, err := theRunner.otherJobRunning(ctx, s, "publish-"+s.Spec.ID+"-2")
	if _, now, _, _ := gh.state(); err != nil || busy || now != revoked+1 {
		t.Errorf("busy %v, err %v, revoked %d more, want false, nil, 1", busy, err, now-revoked)
	}
	if err := k8s.Get(ctx, client.ObjectKeyFromObject(job), &batchv1.Job{}); !apierrors.IsNotFound(err) {
		t.Errorf("the superseded Job is still there: %v", err)
	}
}

// A Job the cache still shows suspended but that changed since (released, say) is not deleted: the
// delete holds only for the version judged suspended.
func TestSupersedeLeavesAJobThatChanged(t *testing.T) {
	ctx := context.Background()
	s := newSession(t, publishedRepo)
	stale := s.DeepCopy()
	stale.Status.Pushes = 1
	job := newPublishJob(stale, options, "file://"+upstreamDir())
	if err := k8s.Create(ctx, job); err != nil {
		t.Fatal(err)
	}
	judged := job.DeepCopy()
	before := job.DeepCopy()
	no := false
	job.Spec.Suspend = &no // released since the cache read
	if err := k8s.Patch(ctx, job, client.MergeFrom(before)); err != nil {
		t.Fatal(err)
	}
	gone, err := theRunner.supersede(ctx, judged)
	if err != nil || gone {
		t.Errorf("gone %v, err %v; want a Job that changed left alone", gone, err)
	}
	if err := k8s.Get(ctx, client.ObjectKeyFromObject(job), &batchv1.Job{}); err != nil {
		t.Errorf("the changed Job was deleted: %v", err)
	}
}
