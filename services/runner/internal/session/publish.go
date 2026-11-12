package session

import (
	"context"
	"fmt"
	"log/slog"
	"net/url"
	"strings"
	"time"

	batchv1 "k8s.io/api/batch/v1"
	corev1 "k8s.io/api/core/v1"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/controller/controllerutil"

	"github.com/rangelight/rangelight/services/runner/internal/github"
)

// Publishing (ADR 0011: the push upstream is a control-plane job, ADR 0040 §3: the GitHub App).
// Each push of a session's work branch gets one Job, publish-<id>-<n> (the n-th push), in the
// runner's namespace: services/git-mirror/publish.sh, in the mirror's image, copies the branch from
// the mirror to its upstream. The runner never moves git data itself, so one lane's huge push can
// only fail its own Job. Reconcile only creates, releases and reads Jobs; it never waits on one.
const (
	publishDeadline  = 600                           // seconds a publish Job may run
	publishTTL       = 600                           // seconds a finished Job (and its token Secret) is kept
	publishRetries   = 2                             // the Job's own retries of a failed push, with Kubernetes' backoff
	publishGaveUp    = 3                             // publish.sh's exit code when GitHub refuses the branch for good
	publishTokenDir  = "/var/run/rangelight/publish" //nolint:gosec // a mount path, not a credential
	jobNameLabel     = "batch.kubernetes.io/job-name"
	nothingToPublish = "nothing to publish"
	// publishingOff is the status message of a session that pushed while no GitHub App is configured.
	publishingOff = "publishing is off: no GitHub App configured (githubApp.secretRef)"
	publishUID    = 10001 // the git mirror image's user
)

// publishDue moves the publish of s's last pushed commit on by one step, and returns how long to
// wait before trying again after a failure (0: the Job's own events bring the Session back).
func (r *Reconciler) publishDue(ctx context.Context, s *Session) time.Duration {
	if r.GitHub == nil {
		if s.Status.Pushes > 0 {
			s.Status.PublishMessage = publishingOff
		}
		return 0
	}
	if s.Status.Pushes == s.Status.PublishedPush {
		return 0
	}
	if wait := r.pub.wait(s.Spec.ID); wait > 0 {
		return wait
	}
	upstream, owner, repo, err := r.upstream(s.Spec.Repo)
	if err != nil {
		r.publishedAs(s, "Not published: "+err.Error())
		return 0
	}
	job, err := r.publishJob(ctx, s, upstream, owner, repo)
	if err != nil {
		return r.retryLater(ctx, s, err)
	}
	if job == nil {
		return 0 // the Job of an earlier commit is still running
	}
	end := finished(job)
	if end == "" || r.pub.is(r.pub.handled, job.Name) {
		return 0 // running, or ended and acted on (this read is older than that status write)
	}
	if end == batchv1.JobFailed {
		r.pub.mark(r.pub.handled, job.Name)
		r.revoke(ctx, job.Namespace, job.Name)
		r.publishedAs(s, "Not published: "+r.jobMessage(ctx, job)+". The next push tries again.")
		return 0
	}
	if r.jobMessage(ctx, job) == nothingToPublish {
		r.pub.mark(r.pub.handled, job.Name)
		r.revoke(ctx, job.Namespace, job.Name)
		r.publishedAs(s, "")
		return 0
	}
	pr, err := r.openPullRequest(ctx, s, job.Name, owner, repo)
	if err != nil {
		return r.retryLater(ctx, s, err)
	}
	r.pub.mark(r.pub.handled, job.Name)
	s.Status.PullRequestURL = pr
	r.publishedAs(s, "")
	return 0
}

// publishedAs ends the publish of the last pushed commit, with msg saying why when it failed. The
// next push publishes again.
func (r *Reconciler) publishedAs(s *Session, msg string) {
	s.Status.PublishedCommit, s.Status.PublishedPush = s.Status.WorkCommit, s.Status.Pushes
	s.Status.PublishMessage = oneLine(msg)
	r.pub.reset(s.Spec.ID)
}

// retryLater records a failure that the backoff retries, in the status message too.
func (r *Reconciler) retryLater(ctx context.Context, s *Session, err error) time.Duration {
	wait := r.pub.fail(s.Spec.ID)
	slog.WarnContext(ctx, "publish failed; retrying", "session_id", s.Spec.ID, "error", err.Error(), "retry_in", wait.String())
	s.Status.PublishMessage = oneLine("Publishing the work branch failed, retrying: " + err.Error())
	return wait
}

// publishJob is the Job of s's last pushed commit, created (suspended) if missing, and released
// once its token Secret exists. nil: another of s's Jobs is still running, and this one waits.
func (r *Reconciler) publishJob(ctx context.Context, s *Session, upstream, owner, repo string) (*batchv1.Job, error) {
	want := newPublishJob(s, r.Options, upstream)
	var job batchv1.Job
	err := r.Get(ctx, client.ObjectKeyFromObject(want), &job)
	if apierrors.IsNotFound(err) {
		if busy, err := r.otherJobRunning(ctx, s, want.Name); err != nil || busy {
			return nil, err
		}
		if err := controllerutil.SetControllerReference(s, want, r.Scheme()); err != nil {
			return nil, fmt.Errorf("publish job: %w", err)
		}
		if err := r.Create(ctx, want); err != nil {
			return nil, fmt.Errorf("publish job: %w", err)
		}
		job = *want
	} else if err != nil {
		return nil, fmt.Errorf("publish job: %w", err)
	}
	if job.Spec.Suspend != nil && *job.Spec.Suspend {
		return &job, r.release(ctx, &job, owner, repo)
	}
	return &job, nil
}

// otherJobRunning reports whether another publish Job of s is still running, so two never push
// the branch at once. It revokes the tokens of those that ended, and removes an older Job that never
// started (still suspended: its token failed, or the runner stopped before releasing it), which the
// newer push supersedes; nothing would ever release it.
func (r *Reconciler) otherJobRunning(ctx context.Context, s *Session, name string) (bool, error) {
	var jobs batchv1.JobList
	if err := r.List(ctx, &jobs, client.InNamespace(s.Namespace), client.MatchingLabels{LabelSession: s.Spec.ID}); err != nil {
		return false, fmt.Errorf("publish jobs: %w", err)
	}
	busy := false
	for i := range jobs.Items {
		j := &jobs.Items[i]
		switch {
		case j.Name == name:
		case j.Spec.Suspend != nil && *j.Spec.Suspend && finished(j) == "":
			gone, err := r.supersede(ctx, j)
			if err != nil {
				return false, err
			}
			busy = busy || !gone
		case finished(j) == "":
			busy = true
		default:
			r.revoke(ctx, j.Namespace, j.Name)
		}
	}
	return busy, nil
}

// supersede deletes j, a Job the cache shows suspended, then revokes its token. The delete holds only
// for that version of j: if it changed since (it was released: it may be pushing now), it is left
// alone, and false says to wait for it (its events bring the Session back).
func (r *Reconciler) supersede(ctx context.Context, j *batchv1.Job) (bool, error) {
	rv := j.ResourceVersion
	err := r.Delete(ctx, j, client.PropagationPolicy(metav1.DeletePropagationBackground),
		&client.DeleteOptions{Preconditions: &metav1.Preconditions{UID: &j.UID, ResourceVersion: &rv}})
	switch {
	case apierrors.IsConflict(err):
		return false, nil
	case client.IgnoreNotFound(err) != nil:
		return false, fmt.Errorf("superseded publish job: %w", err)
	}
	r.revoke(ctx, j.Namespace, j.Name)
	return true, nil
}

// release gives the Job an installation token, in a Secret the Job owns (so it goes with the Job),
// and starts it.
// A Secret already there (a restart after it was made) keeps its token: a second is never minted.
func (r *Reconciler) release(ctx context.Context, job *batchv1.Job, owner, repo string) error {
	token, err := r.heldToken(ctx, job.Namespace, job.Name)
	if err != nil {
		return err
	}
	if token == "" {
		if token, err = r.GitHub.Token(ctx, owner, repo); err != nil {
			return fmt.Errorf("GitHub App: %w", err)
		}
		secret := &corev1.Secret{
			ObjectMeta: metav1.ObjectMeta{Name: job.Name, Namespace: job.Namespace, Labels: job.Labels},
			Data:       map[string][]byte{"token": []byte(token)},
		}
		if err := controllerutil.SetControllerReference(job, secret, r.Scheme()); err != nil {
			return fmt.Errorf("publish token: %w", err)
		}
		if err := r.Create(ctx, secret); err != nil {
			r.revokeToken(ctx, job.Name, token)
			return fmt.Errorf("publish token: %w", err)
		}
	}
	r.pub.remember(job.Name, token)
	before := job.DeepCopy()
	no := false
	job.Spec.Suspend = &no
	if err := r.Patch(ctx, job, client.MergeFrom(before)); err != nil {
		return fmt.Errorf("publish job: %w", err)
	}
	return nil
}

// openPullRequest finds or opens the work branch's pull request, then revokes the token.
func (r *Reconciler) openPullRequest(ctx context.Context, s *Session, job, owner, repo string) (string, error) {
	token, err := r.heldToken(ctx, s.Namespace, job)
	if err != nil {
		return "", err
	}
	r.pub.forget(job)
	if token == "" { // the Secret is gone already (the Job's TTL)
		if token, err = r.GitHub.Token(ctx, owner, repo); err != nil {
			return "", fmt.Errorf("GitHub App: %w", err)
		}
	}
	defer r.revokeToken(ctx, job, token)
	pr, err := r.GitHub.Open(ctx, token, github.PullRequest{
		Owner: owner, Repo: repo, Head: workBranch(s.Spec.ID), Base: s.Spec.BaseRef,
		Title: s.Spec.Ticket.Key + ": " + s.Spec.Ticket.Title, Body: prBody(s.Spec.Ticket.Key, s.Spec.Ticket.URL),
	})
	if err != nil {
		return "", fmt.Errorf("pull request: %w", err)
	}
	return pr, nil
}

// heldToken is a Job's installation token: the one in memory, else the one in its Secret (after
// a restart), else "". The Secret is read past the cache: the runner caches no Secrets.
func (r *Reconciler) heldToken(ctx context.Context, namespace, job string) (string, error) {
	if r.pub.is(r.pub.revoked, job) {
		return "", nil
	}
	if token := r.pub.token(job); token != "" {
		return token, nil
	}
	var secret corev1.Secret
	err := r.Reader.Get(ctx, client.ObjectKey{Namespace: namespace, Name: job}, &secret)
	if apierrors.IsNotFound(err) {
		return "", nil
	}
	if err != nil {
		return "", fmt.Errorf("publish token: %w", err)
	}
	return string(secret.Data["token"]), nil
}

// revoke ends the token of a Job that has ended, wherever the runner finds it.
func (r *Reconciler) revoke(ctx context.Context, namespace, job string) {
	token, err := r.heldToken(ctx, namespace, job)
	r.pub.forget(job)
	if err != nil {
		slog.WarnContext(ctx, "publish token not read; it expires within the hour", "job", job, "error", err.Error())
	}
	if token != "" {
		r.revokeToken(ctx, job, token)
	}
}

// revokeToken revokes token, and marks the Job's Secret as holding a dead one, so a retry of the
// pull request mints a fresh token instead.
func (r *Reconciler) revokeToken(ctx context.Context, job, token string) {
	r.pub.mark(r.pub.revoked, job)
	if err := r.GitHub.Revoke(ctx, token); err != nil {
		slog.WarnContext(ctx, "publish token not revoked; it expires within the hour", "job", job, "error", err.Error())
	}
}

// revokeRunning revokes the tokens of s's publish Jobs that haven't ended, before s (and with it
// its Jobs and their Secrets) goes.
func (r *Reconciler) revokeRunning(ctx context.Context, s *Session) error {
	if r.GitHub == nil {
		return nil
	}
	var jobs batchv1.JobList
	if err := r.List(ctx, &jobs, client.InNamespace(s.Namespace), client.MatchingLabels{LabelSession: s.Spec.ID}); err != nil {
		return fmt.Errorf("publish jobs: %w", err)
	}
	for i := range jobs.Items {
		if finished(&jobs.Items[i]) == "" {
			r.revoke(ctx, s.Namespace, jobs.Items[i].Name)
		}
	}
	return nil
}

// jobMessage is what the Job's script said last (its termination message), or why the Job failed.
func (r *Reconciler) jobMessage(ctx context.Context, job *batchv1.Job) string {
	var pods corev1.PodList
	if err := r.List(ctx, &pods, client.InNamespace(job.Namespace), client.MatchingLabels{jobNameLabel: job.Name}); err == nil {
		if msg := lastTerminationMessage(pods.Items); msg != "" {
			return msg
		}
	}
	for _, c := range job.Status.Conditions {
		if c.Type == batchv1.JobFailed && c.Status == corev1.ConditionTrue {
			return "the publish Job failed: " + c.Reason
		}
	}
	return "the publish Job said nothing"
}

// lastTerminationMessage is the message of the container that ended last, or "".
func lastTerminationMessage(pods []corev1.Pod) string {
	var last metav1.Time
	msg := ""
	for _, p := range pods {
		for _, c := range p.Status.ContainerStatuses {
			if t := c.State.Terminated; t != nil && t.Message != "" && !t.FinishedAt.Before(&last) {
				last, msg = t.FinishedAt, strings.TrimSpace(t.Message)
			}
		}
	}
	return msg
}

// finished is JobComplete or JobFailed once the Job has ended, else "".
func finished(job *batchv1.Job) batchv1.JobConditionType {
	for _, c := range job.Status.Conditions {
		if (c.Type == batchv1.JobComplete || c.Type == batchv1.JobFailed) && c.Status == corev1.ConditionTrue {
			return c.Type
		}
	}
	return ""
}

// upstream is the git URL a repo's work branches are pushed to, and its GitHub owner and name: the
// last two elements of its path.
func (r *Reconciler) upstream(name string) (string, string, string, error) {
	upstream, ok := r.Upstreams[name]
	if !ok {
		return "", "", "", fmt.Errorf("no upstream for repo %s", name)
	}
	u, err := url.Parse(upstream)
	if err != nil {
		return "", "", "", fmt.Errorf("upstream of %s: %w", name, err)
	}
	parts := strings.Split(strings.TrimSuffix(strings.Trim(u.Path, "/"), ".git"), "/")
	if len(parts) < 2 {
		return "", "", "", fmt.Errorf("upstream of %s is not <owner>/<repo>", name)
	}
	return upstream, parts[len(parts)-2], parts[len(parts)-1], nil
}

// workBranch is the one branch of a session the runner publishes: rangelight/<id>/work.
func workBranch(id string) string { return "rangelight/" + id + "/work" }

// markdownSafe percent-encodes what could end a Markdown link's target or start another.
var markdownSafe = strings.NewReplacer("(", "%28", ")", "%29", "[", "%5B", "]", "%5D", "<", "%3C", ">", "%3E",
	"\\", "%5C", "`", "%60", `"`, "%22", "'", "%27", "*", "%2A", "!", "%21")

// prBody links the ticket: only a page on https://linear.app, escaped, else the key alone.
func prBody(key, ticketURL string) string {
	u, err := url.Parse(ticketURL)
	if err != nil || u.Scheme != "https" || u.Host != "linear.app" || u.User != nil || u.Opaque != "" {
		return "Linear: " + key
	}
	return "Linear: [" + key + "](" + markdownSafe.Replace(u.String()) + ")"
}
