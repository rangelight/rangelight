package session

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	batchv1 "k8s.io/api/batch/v1"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"
)

// upstreamDir is the bare upstream repo of publishedRepo.
func upstreamDir() string {
	return filepath.Join(mirror.Root, "upstream", "acme", publishedRepo+".git")
}

// sessionPush stores commit on the session's branch in its repo on the mirror, as the session's
// push would, and returns the commit.
func sessionPush(t *testing.T, s *Session, branch, file string) string {
	t.Helper()
	bare := filepath.Join(mirror.Root, "sessions", s.Spec.ID+".git")
	if _, err := os.Stat(bare); err != nil {
		runGit(t, "", "init", "-q", "--bare", bare)
	}
	work := t.TempDir()
	runGit(t, "", "init", "-q", "-b", "main", work)
	if err := os.WriteFile(filepath.Join(work, file), []byte(file), 0o600); err != nil {
		t.Fatal(err)
	}
	runGit(t, work, "add", "-A")
	runGit(t, work, "-c", "user.name=test", "-c", "user.email=test@example.com", "commit", "-q", "-m", file)
	runGit(t, work, "push", "-q", "-f", bare, "HEAD:refs/heads/"+branch)
	return runGit(t, work, "rev-parse", "HEAD")
}

// reportPush POSTs a push report to /git/pushed, as the mirror's post-receive hook does.
func reportPush(header, session, ref, commit string) int {
	raw, _ := json.Marshal(map[string]string{"session": session, "ref": ref, "commit": commit})
	req := httptest.NewRequest(http.MethodPost, "/git/pushed", bytes.NewReader(raw))
	if header != "" {
		req.Header.Set("Authorization", header)
	}
	rec := httptest.NewRecorder()
	NewServer(":0", &GitAuth{Client: mgr.GetClient(), Cache: mgr.GetCache(), Reader: mgr.GetAPIReader(), Namespace: runnerNS}).Handler.ServeHTTP(rec, req)
	return rec.Code
}

// laneToken is the projected token of the session's agent pod, once the runner made it.
func laneToken(t *testing.T, s *Session) string {
	t.Helper()
	pod := &corev1.Pod{ObjectMeta: metav1.ObjectMeta{Name: agentName, Namespace: Namespace(s)}}
	get(t, pod)
	return token(t, Namespace(s), pod, MirrorAudience)
}

func reload(t *testing.T, s *Session) *Session {
	t.Helper()
	var got Session
	if err := k8s.Get(context.Background(), client.ObjectKeyFromObject(s), &got); err != nil {
		t.Fatal(err)
	}
	return &got
}

// upstreamRef is the commit of the session's work branch upstream, or "".
func upstreamRef(t *testing.T, s *Session) string {
	t.Helper()
	out, _ := exec.Command("git", "--git-dir", upstreamDir(), "rev-parse", "--verify", "-q", "refs/heads/"+workBranch(s.Spec.ID)).Output() //nolint:gosec // the tests' own git calls
	return strings.TrimSpace(string(out))
}

// pushed stores a new commit on the session's work branch, as its push would, and reports it with
// the lane's token, as the mirror's hook would.
func pushed(t *testing.T, s *Session, token, file string) string {
	t.Helper()
	commit := sessionPush(t, s, workBranch(s.Spec.ID), file)
	if code := reportPush("Bearer "+token, s.Spec.ID, "refs/heads/"+workBranch(s.Spec.ID), commit); code != http.StatusAccepted {
		t.Fatalf("push report: status %d", code)
	}
	return commit
}

// releasedJob waits for the publish Job of the session's last push, released with its token Secret.
func releasedJob(t *testing.T, s *Session) *batchv1.Job {
	t.Helper()
	name := fmt.Sprintf("publish-%s-%d", s.Spec.ID, reload(t, s).Status.Pushes)
	job := &batchv1.Job{ObjectMeta: metav1.ObjectMeta{Name: name, Namespace: runnerNS}}
	eventually(t, "the publish job "+name, func() error {
		if err := k8s.Get(context.Background(), client.ObjectKeyFromObject(job), job); err != nil {
			return err
		}
		if job.Spec.Suspend == nil || *job.Spec.Suspend {
			return errors.New("still suspended")
		}
		return nil
	})
	return job
}

// runJob plays the Job controller and the kubelet: it runs the Job's publish.sh here, with the
// Job's own arguments and its Secret's token, then records the pod's end and the Job's.
func runJob(t *testing.T, job *batchv1.Job) {
	t.Helper()
	var secret corev1.Secret
	if err := k8s.Get(context.Background(), client.ObjectKeyFromObject(job), &secret); err != nil {
		t.Fatal(err)
	}
	dir := t.TempDir()
	files := map[string][]byte{"github": secret.Data["token"], "mirror": []byte(runnerToken + "\n"), "log": nil}
	for name, data := range files {
		if err := os.WriteFile(filepath.Join(dir, name), data, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	c := job.Spec.Template.Spec.Containers[0]
	// env(1) adds the Job's files to the tests' own environment.
	args := append([]string{"HOME=" + dir, "GIT_ALLOW_PROTOCOL=file:http", "RANGELIGHT_GITHUB_TOKEN_FILE=" + filepath.Join(dir, "github"),
		"RANGELIGHT_MIRROR_TOKEN_FILE=" + filepath.Join(dir, "mirror"), "RANGELIGHT_TERMINATION_LOG=" + filepath.Join(dir, "log"),
		"../../../git-mirror/publish.sh"}, c.Command[1:]...)
	out, err := exec.Command("env", args...).CombinedOutput() //nolint:gosec // the Job's own arguments
	msg, _ := os.ReadFile(filepath.Join(dir, "log"))          //nolint:gosec // the test's own temporary file
	if err != nil {
		t.Fatalf("publish.sh: %v\n%s", err, out)
	}
	finishJob(t, job, true, string(msg))
}

// finishJob records the Job's one pod as ended with message, and the Job as complete or failed.
func finishJob(t *testing.T, job *batchv1.Job, ok bool, message string) {
	t.Helper()
	ctx := context.Background()
	ensureServiceAccount(t, runnerNS)
	pod := &corev1.Pod{
		ObjectMeta: metav1.ObjectMeta{Name: job.Name, Namespace: runnerNS, Labels: map[string]string{jobNameLabel: job.Name, LabelSession: job.Labels[LabelSession]}},
		Spec:       corev1.PodSpec{ServiceAccountName: "default", Containers: []corev1.Container{{Name: "publish", Image: testImage}}},
	}
	if err := k8s.Create(ctx, pod); err != nil {
		t.Fatal(err)
	}
	now := metav1.Now()
	pod.Status.ContainerStatuses = []corev1.ContainerStatus{{Name: "publish", Image: testImage,
		State: corev1.ContainerState{Terminated: &corev1.ContainerStateTerminated{Message: message, FinishedAt: now}}}}
	if err := k8s.Status().Update(ctx, pod); err != nil {
		t.Fatal(err)
	}
	job.Status.StartTime = &now
	end := []batchv1.JobConditionType{batchv1.JobFailureTarget, batchv1.JobFailed}
	if ok {
		job.Status.Succeeded, job.Status.CompletionTime = 1, &now
		end = []batchv1.JobConditionType{batchv1.JobSuccessCriteriaMet, batchv1.JobComplete}
	} else {
		job.Status.Failed = 1
	}
	for _, c := range end {
		job.Status.Conditions = append(job.Status.Conditions, batchv1.JobCondition{Type: c, Status: corev1.ConditionTrue, LastTransitionTime: now, Reason: "Test"})
	}
	if err := k8s.Status().Update(ctx, job); err != nil {
		t.Fatal(err)
	}
}

// published waits until commit is the session's work branch upstream, its pull request recorded.
func published(t *testing.T, s *Session, commit string) {
	t.Helper()
	eventually(t, "published "+commit, func() error {
		got := reload(t, s).Status
		if got.PublishedCommit != commit || got.PullRequestURL == "" || upstreamRef(t, s) != commit || got.PublishMessage != "" {
			return fmt.Errorf("status %+v, upstream %q", got, upstreamRef(t, s))
		}
		return nil
	})
	// The upstream is shared by the tests' sessions: of this one's, only the work branch.
	if refs := runGit(t, "", "--git-dir", upstreamDir(), "for-each-ref", "--format=%(refname)", "refs/heads/rangelight/"+s.Spec.ID+"/"); refs != "refs/heads/"+workBranch(s.Spec.ID) {
		t.Errorf("upstream refs %q, want only the work branch", refs)
	}
}

// publishMessage waits until the session's publish message says want.
func publishMessage(t *testing.T, s *Session, want string) *Session {
	t.Helper()
	var got *Session
	eventually(t, "publish message "+want, func() error {
		if got = reload(t, s); !strings.Contains(got.Status.PublishMessage, want) {
			return fmt.Errorf("publish message %q", got.Status.PublishMessage)
		}
		return nil
	})
	return got
}

// A report with no token, another session's token, or naming another session or ref records nothing.
func refusesOtherReports(t *testing.T, s, other *Session, own, theirs string) {
	t.Helper()
	ref := "refs/heads/" + workBranch(s.Spec.ID)
	commit := sessionPush(t, s, "rangelight/"+s.Spec.ID+"/other", "other")
	refusals := []struct {
		name, header, session, ref string
		want                       int
	}{
		{"no token", "", s.Spec.ID, ref, http.StatusUnauthorized},
		{"not a token", "Bearer nope", s.Spec.ID, ref, http.StatusUnauthorized},
		{"another session's token", "Bearer " + theirs, s.Spec.ID, ref, http.StatusForbidden},
		{"naming another session", "Bearer " + own, other.Spec.ID, "refs/heads/" + workBranch(other.Spec.ID), http.StatusForbidden},
		{"another branch", "Bearer " + own, s.Spec.ID, "refs/heads/rangelight/" + s.Spec.ID + "/other", http.StatusForbidden},
		{"the base branch", "Bearer " + own, s.Spec.ID, "refs/heads/main", http.StatusForbidden},
	}
	eventually(t, "the lane's token is accepted", func() error {
		if rec := ask("Bearer " + own); rec.Code != http.StatusOK {
			return fmt.Errorf("status %d", rec.Code)
		}
		return nil
	})
	for _, tt := range refusals {
		if code := reportPush(tt.header, tt.session, tt.ref, commit); code != tt.want {
			t.Errorf("%s: status %d, want %d", tt.name, code, tt.want)
		}
	}
	if got := reload(t, s).Status.WorkCommit + reload(t, other).Status.WorkCommit; got != "" {
		t.Fatalf("a refused report was recorded: %q", got)
	}
}

// The publish Job: restricted, the runner's identity at the mirror only, bounded, its token in a
// Secret the Job owns.
func checkJob(t *testing.T, s *Session, job *batchv1.Job) {
	t.Helper()
	p := job.Spec.Template.Spec
	c := p.Containers[0]
	if p.ServiceAccountName != runnerAccount || p.AutomountServiceAccountToken == nil || *p.AutomountServiceAccountToken ||
		*job.Spec.BackoffLimit != publishRetries || *job.Spec.ActiveDeadlineSeconds != publishDeadline ||
		*job.Spec.TTLSecondsAfterFinished != publishTTL || job.Spec.PodFailurePolicy.Rules[0].OnExitCodes.Values[0] != publishGaveUp ||
		c.Image != options.PublishImage || *c.SecurityContext.ReadOnlyRootFilesystem != true || c.Resources.Limits.Memory().IsZero() ||
		fmt.Sprint(c.Command) != fmt.Sprint([]string{"/opt/git-mirror/publish.sh", s.Spec.ID, options.MirrorURL, "file://" + upstreamDir()}) {
		t.Errorf("publish job %+v", job.Spec)
	}
	if len(job.OwnerReferences) != 1 || job.OwnerReferences[0].Name != s.Name {
		t.Errorf("the job isn't the Session's: %v", job.OwnerReferences)
	}
	var secret corev1.Secret
	if err := k8s.Get(context.Background(), client.ObjectKeyFromObject(job), &secret); err != nil {
		t.Fatal(err)
	}
	if string(secret.Data["token"]) != ghToken || len(secret.OwnerReferences) != 1 || secret.OwnerReferences[0].UID != job.UID {
		t.Errorf("the token Secret: owners %v", secret.OwnerReferences)
	}
}

// RAN-55: a push of the work branch is published by a Job and opens one pull request; later pushes
// update it; the token is revoked once used.
func TestPublishesTheWorkBranch(t *testing.T) {
	const url = "https://linear.app/example/issue/RAN-55"
	demoRepo(t, publishedRepo)
	s := newSession(t, publishedRepo, func(s *Session) { s.Spec.Ticket.URL = url })
	other := newSession(t, publishedRepo)
	own := laneToken(t, s)
	refusesOtherReports(t, s, other, own, laneToken(t, other))

	first := pushed(t, s, own, "one")
	job := releasedJob(t, s)
	checkJob(t, s, job)
	runJob(t, job)
	published(t, s, first)
	mirror.mu.Lock()
	sent := mirror.auth["sessions"] // the Job's requests for /sessions/<id>.git
	mirror.mu.Unlock()
	if got, want := sent, "Basic "+base64.StdEncoding.EncodeToString([]byte("runner:"+runnerToken)); got != want {
		t.Errorf("the job's mirror credentials %q, want the runner's token", got)
	}
	opened, revoked, asks, bodies := gh.state()
	want := map[string]string{"title": s.Spec.Ticket.Key + ": A title", "head": workBranch(s.Spec.ID), "base": "main", "body": "Linear: [" + s.Spec.Ticket.Key + "](" + url + ")"}
	if opened != 1 || len(bodies) != 1 || fmt.Sprint(bodies[0]) != fmt.Sprint(want) || revoked < 1 {
		t.Errorf("opened %d, revoked %d: %v, want %v", opened, revoked, bodies, want)
	}
	if len(asks) == 0 || asks[0] != `{"permissions":{"contents":"write","pull_requests":"write"},"repositories":["published"]}` {
		t.Errorf("token requests %v", asks)
	}

	// A later push updates the same pull request (the restart case is the same lookup by head branch).
	prURL := reload(t, s).Status.PullRequestURL
	second := pushed(t, s, own, "two")
	runJob(t, releasedJob(t, s))
	published(t, s, second)
	if opened, _, _, _ := gh.state(); opened != 1 || reload(t, s).Status.PullRequestURL != prURL {
		t.Errorf("opened %d pull requests, URL %q, want 1, %q", opened, reload(t, s).Status.PullRequestURL, prURL)
	}
	t.Run("a token error is retried with backoff and reported", func(t *testing.T) { retriesTokenErrors(t, s, own) })
	t.Run("a workflow change GitHub refuses is given up and reported", func(t *testing.T) { givesUpOnWorkflows(t, s, own) })
	t.Run("a commit pushed again after another (A, B, A) is published again", func(t *testing.T) { republishesAnEarlierCommit(t, s, own) })
}

func retriesTokenErrors(t *testing.T, s *Session, own string) {
	gh.setFailTokens(true)
	third := pushed(t, s, own, "three")
	publishMessage(t, s, "Publishing the work branch failed, retrying: GitHub App: installation token for acme/published: GitHub answered 500: token service down")
	gh.setFailTokens(false)
	runJob(t, releasedJob(t, s))
	published(t, s, third)
}

func republishesAnEarlierCommit(t *testing.T, s *Session, own string) {
	a := pushed(t, s, own, "a")
	runJob(t, releasedJob(t, s))
	published(t, s, a)
	b := pushed(t, s, own, "b")
	runJob(t, releasedJob(t, s))
	published(t, s, b)
	// The lane force-pushes its work branch back to A.
	runGit(t, "", "--git-dir", filepath.Join(mirror.Root, "sessions", s.Spec.ID+".git"), "update-ref", "refs/heads/"+workBranch(s.Spec.ID), a)
	if code := reportPush("Bearer "+own, s.Spec.ID, "refs/heads/"+workBranch(s.Spec.ID), a); code != http.StatusAccepted {
		t.Fatalf("push report: status %d", code)
	}
	runJob(t, releasedJob(t, s))
	published(t, s, a)
	if opened, _, _, _ := gh.state(); opened != 1 {
		t.Errorf("opened %d pull requests, want the one", opened)
	}
}

func givesUpOnWorkflows(t *testing.T, s *Session, own string) {
	fourth := pushed(t, s, own, "four")
	job := releasedJob(t, s)
	finishJob(t, job, false, "GitHub refused it: the work branch changes .github/workflows, which the GitHub App may not push")
	got := publishMessage(t, s, "Not published: GitHub refused it: the work branch changes .github/workflows")
	if got.Status.PublishedCommit != fourth {
		t.Errorf("published commit %q, want %q (given up, not retried)", got.Status.PublishedCommit, fourth)
	}
	var jobs batchv1.JobList
	if err := k8s.List(context.Background(), &jobs, client.InNamespace(runnerNS), client.MatchingLabels{LabelSession: s.Spec.ID}); err != nil {
		t.Fatal(err)
	}
	if len(jobs.Items) != 4 {
		t.Errorf("%d publish jobs, want one per push (4)", len(jobs.Items))
	}
}

// Nothing on the mirror publishes nothing; a repo without an upstream is reported, and no Job runs.
func TestPublishesNothingWithoutWorkOrUpstream(t *testing.T) {
	s := newSession(t, publishedRepo)
	own := laneToken(t, s)
	sessionPush(t, s, "rangelight/"+s.Spec.ID+"/other", "other") // its repo, without a work branch
	commit := strings.Repeat("ab", 20)
	eventually(t, "the push report", func() error {
		if code := reportPush("Bearer "+own, s.Spec.ID, "refs/heads/"+workBranch(s.Spec.ID), commit); code != http.StatusAccepted {
			return fmt.Errorf("status %d", code)
		}
		return nil
	})
	opened, _, _, _ := gh.state()
	runJob(t, releasedJob(t, s))
	eventually(t, "nothing published", func() error {
		if got := reload(t, s).Status; got.PublishedCommit != commit || got.PullRequestURL != "" || got.PublishMessage != "" {
			return fmt.Errorf("status %+v", got)
		}
		return nil
	})
	if now, _, _, _ := gh.state(); now != opened {
		t.Errorf("a pull request was opened for nothing")
	}

	demoRepo(t, "no-upstream")
	n := newSession(t, "no-upstream")
	pushed(t, n, laneToken(t, n), "work")
	publishMessage(t, n, "Not published: no upstream for repo no-upstream")
	var jobs batchv1.JobList
	if err := k8s.List(context.Background(), &jobs, client.MatchingLabels{LabelSession: n.Spec.ID}); err != nil || len(jobs.Items) != 0 {
		t.Errorf("jobs %d, %v", len(jobs.Items), err)
	}
}

// forgetTokens plays a runner restart: the tokens it held in memory are gone.
func forgetTokens() {
	theRunner.pub.mu.Lock()
	defer theRunner.pub.mu.Unlock()
	theRunner.pub.tokens = map[string]string{}
}

// suspend plays a crash after the token Secret was made, before the Job was released.
func suspend(t *testing.T, job *batchv1.Job) {
	t.Helper()
	before := job.DeepCopy()
	yes := true
	job.Spec.Suspend = &yes
	if err := k8s.Patch(context.Background(), job, client.MergeFrom(before)); err != nil {
		t.Fatal(err)
	}
}

// A restart between the token Secret and the Job's release mints no second token, and the one in the
// Secret is revoked when the Job ends; a Session deleted mid-Job has its token revoked too.
func TestARestartReusesAndRevokesTheSecretsToken(t *testing.T) {
	s := newSession(t, publishedRepo)
	own := laneToken(t, s)
	_, revokedBefore, asksBefore, _ := gh.state()
	commit := pushed(t, s, own, "restart")
	job := releasedJob(t, s)
	forgetTokens()
	suspend(t, job)
	job = releasedJob(t, s)
	forgetTokens()
	runJob(t, job)
	published(t, s, commit)
	_, revoked, asks, _ := gh.state()
	if len(asks)-len(asksBefore) != 1 || revoked-revokedBefore != 1 {
		t.Errorf("%d tokens minted and %d revoked, want 1 and 1", len(asks)-len(asksBefore), revoked-revokedBefore)
	}

	pushed(t, s, own, "mid-job")
	releasedJob(t, s)
	forgetTokens()
	if err := k8s.Delete(context.Background(), s); err != nil {
		t.Fatal(err)
	}
	gone(t, s)
	if _, now, _, _ := gh.state(); now != revoked+1 {
		t.Errorf("revoked %d, want %d: the deleted Session's running Job keeps its token", now, revoked+1)
	}
}

func TestPullRequestBodyLinksOnlyLinear(t *testing.T) {
	for url, want := range map[string]string{
		"https://linear.app/x/issue/RAN-1/a-title":             "Linear: [RAN-1](https://linear.app/x/issue/RAN-1/a-title)",
		"https://linear.app/x)[Approve](https://evil.example)": "Linear: [RAN-1](https://linear.app/x%29%5BApprove%5D%28https://evil.example%29)",
		"https://linear.app/x/<img src=x>":                     "Linear: [RAN-1](https://linear.app/x/%3Cimg%20src=x%3E)",
		"https://evil.example/x":                               "Linear: RAN-1",
		"https://linear.app.evil.example/x":                    "Linear: RAN-1",
		"https://user@linear.app/x":                            "Linear: RAN-1",
		"http://linear.app/x":                                  "Linear: RAN-1",
		"":                                                     "Linear: RAN-1",
	} {
		if got := prBody("RAN-1", url); got != want {
			t.Errorf("%q: %q, want %q", url, got, want)
		}
	}
}

// The lane's ticket.json is the ticket schema without the url.
func TestTicketFileLeavesOutTheURL(t *testing.T) {
	demoRepo(t, "ticket-url")
	s := newSession(t, "ticket-url", func(s *Session) { s.Spec.Ticket.URL = "https://linear.app/example/issue/RAN-1" })
	cm := &corev1.ConfigMap{ObjectMeta: metav1.ObjectMeta{Name: laneName, Namespace: Namespace(s)}}
	get(t, cm)
	if strings.Contains(cm.Data["ticket.json"], "url") || !strings.Contains(cm.Data["ticket.json"], s.Spec.Ticket.Key) {
		t.Errorf("ticket.json %s", cm.Data["ticket.json"])
	}
}
