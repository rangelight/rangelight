package session

import (
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/resource"
)

// agentPod is the lane's agent (ADR 0015): non-root, read-only root filesystem, no capabilities,
// seccomp RuntimeDefault, no ServiceAccount token but one projected token for the git mirror. The
// only writable places are the worktree, /tmp and the Claude home, each bounded.
func agentPod(s *Session, o Options) *corev1.Pod {
	no, yes, ndots := false, true, "1"
	uid := int64(agentUID)
	expiry := int64(tokenSeconds)
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
				{Name: "git-mirror-token", VolumeSource: corev1.VolumeSource{Projected: &corev1.ProjectedVolumeSource{
					Sources: []corev1.VolumeProjection{{ServiceAccountToken: &corev1.ServiceAccountTokenProjection{
						Audience: MirrorAudience, ExpirationSeconds: &expiry, Path: "token",
					}}},
				}}},
			},
		},
	}
}

// ready reports whether a pod's Ready condition is true.
func ready(p *corev1.Pod) bool {
	for _, c := range p.Status.Conditions {
		if c.Type == corev1.PodReady {
			return c.Status == corev1.ConditionTrue
		}
	}
	return false
}
