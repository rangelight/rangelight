package session

import (
	"fmt"
	"strings"

	batchv1 "k8s.io/api/batch/v1"
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/resource"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
)

// agentPod is the lane's agent (ADR 0015): non-root, read-only root filesystem, no capabilities,
// seccomp RuntimeDefault, no ServiceAccount token but two projected ones: for the git mirror, and
// for NATS, where it signs the lane in to its own terminal only (ADR 0045 amendment). The only
// writable places are the worktree, /tmp and the Claude home, each bounded.
func agentPod(s *Session, o Options) *corev1.Pod {
	no, yes, ndots := false, true, "1"
	uid := int64(agentUID)
	return &corev1.Pod{
		ObjectMeta: objectMeta(agentName, Namespace(s), s.Spec.ID),
		Spec: corev1.PodSpec{
			// The namespace's own account, which may do nothing; the token is only the lane's identity.
			ServiceAccountName:           "default",
			AutomountServiceAccountToken: &no,
			EnableServiceLinks:           &no,
			// Names are looked up as given: the lane's DNS answers only exact names (ciliumPolicy).
			DNSConfig: &corev1.PodDNSConfig{Options: []corev1.PodDNSConfigOption{{Name: "ndots", Value: &ndots}}},
			SecurityContext: &corev1.PodSecurityContext{
				RunAsNonRoot: &yes, RunAsUser: &uid, RunAsGroup: &uid, FSGroup: &uid,
				SeccompProfile: &corev1.SeccompProfile{Type: corev1.SeccompProfileTypeRuntimeDefault},
			},
			Containers: []corev1.Container{{
				Name:  agentName,
				Image: o.AgentImage,
				Env:   agentEnv(s, o),
				SecurityContext: &corev1.SecurityContext{
					RunAsNonRoot: &yes, AllowPrivilegeEscalation: &no, ReadOnlyRootFilesystem: &yes,
					Capabilities: &corev1.Capabilities{Drop: []corev1.Capability{"ALL"}},
				},
				Resources: corev1.ResourceRequirements{
					Requests: corev1.ResourceList{corev1.ResourceCPU: resource.MustParse("500m"), corev1.ResourceMemory: resource.MustParse("1Gi")},
					Limits: corev1.ResourceList{
						corev1.ResourceMemory: resource.MustParse("4Gi"), corev1.ResourceEphemeralStorage: o.EphemeralStorage,
					},
				},
				VolumeMounts: []corev1.VolumeMount{
					{Name: "workspace", MountPath: "/workspace"},
					{Name: "tmp", MountPath: "/tmp"},
					{Name: claudeHome, MountPath: claudeHomeDir},
					{Name: laneName, MountPath: "/etc/rangelight", ReadOnly: true},
					{Name: "git-mirror-token", MountPath: tokenDir, ReadOnly: true},
					{Name: "nats-token", MountPath: natsTokenDir, ReadOnly: true},
				},
			}},
			Volumes: []corev1.Volume{
				{Name: "workspace", VolumeSource: corev1.VolumeSource{EmptyDir: &corev1.EmptyDirVolumeSource{SizeLimit: &o.Workspace}}},
				{Name: "tmp", VolumeSource: corev1.VolumeSource{EmptyDir: &corev1.EmptyDirVolumeSource{SizeLimit: &o.Tmp}}},
				{Name: claudeHome, VolumeSource: corev1.VolumeSource{
					PersistentVolumeClaim: &corev1.PersistentVolumeClaimVolumeSource{ClaimName: claudeHome},
				}},
				{Name: laneName, VolumeSource: corev1.VolumeSource{
					ConfigMap: &corev1.ConfigMapVolumeSource{LocalObjectReference: corev1.LocalObjectReference{Name: laneName}},
				}},
				{Name: "git-mirror-token", VolumeSource: projectedToken(MirrorAudience, tokenSeconds)},
				{Name: "nats-token", VolumeSource: projectedToken(NATSAudience, natsTokenSeconds)},
			},
		},
	}
}

// projectedToken is a volume holding the pod's ServiceAccount token for audience, as the file token,
// valid for seconds (the kubelet renews it before then).
func projectedToken(audience string, seconds int64) corev1.VolumeSource {
	return corev1.VolumeSource{Projected: &corev1.ProjectedVolumeSource{
		Sources: []corev1.VolumeProjection{{ServiceAccountToken: &corev1.ServiceAccountTokenProjection{
			Audience: audience, ExpirationSeconds: &seconds, Path: "token",
		}}},
	}}
}

// agentEnv is the agent's config (configs/agent.default.toml, RANGELIGHT_AGENT_*).
func agentEnv(s *Session, o Options) []corev1.EnvVar {
	return []corev1.EnvVar{
		{Name: "RANGELIGHT_AGENT_SESSION_ID", Value: s.Spec.ID},
		{Name: "RANGELIGHT_AGENT_MIRROR_URL", Value: o.MirrorURL},
		{Name: "RANGELIGHT_AGENT_REPO", Value: s.Spec.Repo},
		{Name: "RANGELIGHT_AGENT_BASE_REF", Value: s.Spec.BaseRef},
		{Name: "RANGELIGHT_AGENT_NATS_URL", Value: o.NATSURL},
		{Name: "RANGELIGHT_AGENT_ORG", Value: o.Org},
		{Name: "CLAUDE_CONFIG_DIR", Value: claudeHomeDir},
		// The root filesystem is read-only: git, node and the dev command write their caches here.
		{Name: "HOME", Value: "/tmp"},
	}
}

// crashLoops is how many restarts of a crash-looping agent container fail its session. The pod
// restarts it (restartPolicy Always: the agent resumes from its checkpoint), so fewer are Starting.
const crashLoops = 3

// podPhase is the session's phase as its agent pod shows it: Hot only while the agent container is
// Ready (and the pod with it); Failed, with the last exit's reason and code, once it crash-loops
// crashLoops times; Starting otherwise, with why it isn't up (an exit, a restart, ImagePullBackOff,
// CreateContainerConfigError, ...) for status.message.
func podPhase(p *corev1.Pod) (Phase, string, error) {
	for _, c := range p.Status.ContainerStatuses {
		if c.Name != agentName {
			continue
		}
		exit := func(t *corev1.ContainerStateTerminated) string {
			if t == nil {
				return "no exit recorded"
			}
			return strings.TrimSpace(fmt.Sprintf("%s (exit code %d) %s", t.Reason, t.ExitCode, t.Message))
		}
		waiting := c.State.Waiting
		switch {
		case waiting != nil && waiting.Reason == "CrashLoopBackOff" && c.RestartCount >= crashLoops:
			return "", "", failed("agent container is crash-looping after %d restarts: %s", c.RestartCount, exit(c.LastTerminationState.Terminated))
		case waiting != nil && waiting.Reason == "CrashLoopBackOff":
			return PhaseStarting, fmt.Sprintf("agent container restarting (%d): %s", c.RestartCount, exit(c.LastTerminationState.Terminated)), nil
		case c.State.Terminated != nil:
			return PhaseStarting, "agent container exited: " + exit(c.State.Terminated), nil
		case waiting != nil && waiting.Reason != "" && waiting.Reason != "ContainerCreating" && waiting.Reason != "PodInitializing":
			return PhaseStarting, fmt.Sprintf("agent container waiting: %s: %s", waiting.Reason, waiting.Message), nil
		case c.Ready && podReady(p):
			return PhaseHot, "", nil
		}
	}
	return PhaseStarting, "", nil
}

// podReady reports whether a pod's Ready condition is true.
func podReady(p *corev1.Pod) bool {
	for _, c := range p.Status.Conditions {
		if c.Type == corev1.PodReady {
			return c.Status == corev1.ConditionTrue
		}
	}
	return false
}

// newPublishJob is the Job that publishes s's last pushed commit (publish.go): suspended until the
// runner has put its token Secret beside it; the git mirror's image running publish.sh as its
// non-root user, read-only root, no capabilities. It is the runner at the mirror through its own
// projected token (audience rangelight-git-mirror only, so it can't call the API server), with no
// automounted token. A refusal GitHub would repeat (exit publishGaveUp) fails it at once; any other
// failure is retried publishRetries times with Kubernetes' backoff.
func newPublishJob(s *Session, o Options, upstream string) *batchv1.Job {
	no, yes := false, true
	uid, expiry := int64(publishUID), int64(tokenSeconds)
	deadline, ttl, retries := int64(publishDeadline), int32(publishTTL), int32(publishRetries)
	name := fmt.Sprintf("publish-%s-%d", s.Spec.ID, s.Status.Pushes)
	meta := objectMeta(name, s.Namespace, s.Spec.ID)
	meta.Labels["app.kubernetes.io/component"] = "publish"
	tmp := resource.MustParse("2Gi")
	return &batchv1.Job{
		ObjectMeta: meta,
		Spec: batchv1.JobSpec{
			Suspend: &yes, BackoffLimit: &retries, ActiveDeadlineSeconds: &deadline, TTLSecondsAfterFinished: &ttl,
			PodFailurePolicy: &batchv1.PodFailurePolicy{Rules: []batchv1.PodFailurePolicyRule{{
				Action:      batchv1.PodFailurePolicyActionFailJob,
				OnExitCodes: &batchv1.PodFailurePolicyOnExitCodesRequirement{Operator: batchv1.PodFailurePolicyOnExitCodesOpIn, Values: []int32{publishGaveUp}},
			}}},
			Template: corev1.PodTemplateSpec{
				ObjectMeta: metav1.ObjectMeta{Labels: meta.Labels},
				Spec: corev1.PodSpec{
					RestartPolicy: corev1.RestartPolicyNever,
					// ponytail: the runner's identity (read-only at the mirror, no API token); ceiling: a git
					// pack-parsing RCE in this pod could read other sessions' repos. Upgrade: a per-session
					// publisher identity at the mirror.
					ServiceAccountName: runnerAccount, AutomountServiceAccountToken: &no, EnableServiceLinks: &no,
					SecurityContext: &corev1.PodSecurityContext{
						RunAsNonRoot: &yes, RunAsUser: &uid, RunAsGroup: &uid,
						SeccompProfile: &corev1.SeccompProfile{Type: corev1.SeccompProfileTypeRuntimeDefault},
					},
					Containers: []corev1.Container{{
						Name:    "publish",
						Image:   o.PublishImage,
						Command: []string{"/opt/git-mirror/publish.sh", s.Spec.ID, o.MirrorURL, upstream},
						Env:     []corev1.EnvVar{{Name: "HOME", Value: "/tmp"}},
						SecurityContext: &corev1.SecurityContext{
							RunAsNonRoot: &yes, AllowPrivilegeEscalation: &no, ReadOnlyRootFilesystem: &yes,
							Capabilities: &corev1.Capabilities{Drop: []corev1.Capability{"ALL"}},
						},
						Resources: corev1.ResourceRequirements{
							Requests: corev1.ResourceList{corev1.ResourceCPU: resource.MustParse("50m"), corev1.ResourceMemory: resource.MustParse("64Mi")},
							Limits:   corev1.ResourceList{corev1.ResourceMemory: resource.MustParse("1Gi")},
						},
						VolumeMounts: []corev1.VolumeMount{
							{Name: "tmp", MountPath: "/tmp"},
							{Name: "token", MountPath: publishTokenDir, ReadOnly: true},
							{Name: "git-mirror-token", MountPath: tokenDir, ReadOnly: true},
						},
					}},
					Volumes: []corev1.Volume{
						{Name: "tmp", VolumeSource: corev1.VolumeSource{EmptyDir: &corev1.EmptyDirVolumeSource{SizeLimit: &tmp}}},
						{Name: "token", VolumeSource: corev1.VolumeSource{Secret: &corev1.SecretVolumeSource{SecretName: name}}},
						{Name: "git-mirror-token", VolumeSource: corev1.VolumeSource{Projected: &corev1.ProjectedVolumeSource{
							Sources: []corev1.VolumeProjection{{ServiceAccountToken: &corev1.ServiceAccountTokenProjection{
								Audience: MirrorAudience, ExpirationSeconds: &expiry, Path: "token",
							}}},
						}}},
					},
				},
			},
		},
	}
}
