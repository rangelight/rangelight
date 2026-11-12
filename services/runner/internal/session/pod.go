package session

import (
	"fmt"
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/resource"
	"strings"
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
				Env: []corev1.EnvVar{
					{Name: "RANGELIGHT_AGENT_SESSION_ID", Value: s.Spec.ID},
					{Name: "RANGELIGHT_AGENT_MIRROR_URL", Value: o.MirrorURL},
					{Name: "RANGELIGHT_AGENT_REPO", Value: s.Spec.Repo},
					{Name: "RANGELIGHT_AGENT_BASE_REF", Value: s.Spec.BaseRef},
					{Name: "CLAUDE_CONFIG_DIR", Value: claudeHomeDir},
					// The root filesystem is read-only: git, node and the dev command write their caches here.
					{Name: "HOME", Value: "/tmp"},
				},
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
