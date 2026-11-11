package session

import (
	"net/url"
	"strconv"
	"strings"

	corev1 "k8s.io/api/core/v1"
	networkingv1 "k8s.io/api/networking/v1"
	"k8s.io/apimachinery/pkg/api/resource"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
	"k8s.io/apimachinery/pkg/runtime/schema"
	"k8s.io/apimachinery/pkg/util/intstr"
)

// Labels the runner sets and reads (ADR 0051: the prefix is rangelight.dev).
const (
	// LabelSession holds the session id on the lane namespace and everything in it.
	LabelSession = "rangelight.dev/session"
	// AnnotationSessionUID on a lane namespace is the UID of the Session that created it. Anyone may set
	// a label to a session's id; only the runner knows the Session object's UID.
	AnnotationSessionUID = "rangelight.dev/session-uid"
	// LabelLane marks a lane namespace; the git mirror admits only these (RAN-17).
	LabelLane = "rangelight.dev/lane"
	// LabelClaudeHome marks a namespace that mounts a Claude home (the cluster guard, ADR 0046).
	LabelClaudeHome = "rangelight.dev/claude-home"
	// LabelMember is the member a claude-home claim belongs to; the StorageClass maps it to a folder.
	LabelMember = "rangelight.dev/member"
)

// The lane's fixed names and the agent pod contract (services/agent, configs/agent.default.toml).
const (
	finalizer       = "rangelight.dev/lane"
	laneName        = "lane"
	agentName       = "agent"
	claudeHome      = "claude-home"
	claudeHomeClass = "rangelight-claude-home"
	claudeHomeDir   = "/var/lib/rangelight/claude-home"
	// MirrorAudience is the audience of the lane's projected token, and the only one /auth/git accepts.
	MirrorAudience = "rangelight-git-mirror"
	tokenDir       = "/var/run/secrets/rangelight/git-mirror" //nolint:gosec // a mount path, not a credential
	tokenSeconds   = 600
	agentUID       = 1000 // the agent image's node user (ADR 0045 §5)
)

// The lane's NATS identity (ADR 0045 amendment): its own terminal, nothing else.
const (
	// NATSAudience is the audience of every NATS client's projected token: the api's auth callout
	// accepts only it. The kubelet renews the token well before its hour is up.
	NATSAudience     = "rangelight-nats"
	natsTokenDir     = "/var/run/secrets/rangelight/nats" //nolint:gosec // a mount path, not a credential
	natsTokenSeconds = 3600
)

// The lane's only in-cluster egress besides the cluster DNS (ADR 0016): the git mirror (RAN-17), and
// the bundled NATS server for the lane terminal (RAN-41).
const (
	systemNamespace = "rangelight-system"
	mirrorApp       = "git-mirror"
	mirrorPort      = 8080
	natsApp         = "nats"
	natsClientPort  = 4222 // NATS's client port, when nats_url names none
)

// natsAddr is the host and client port of a NATS URL.
func natsAddr(u *url.URL) (string, int) {
	if p, err := strconv.Atoi(u.Port()); err == nil {
		return u.Hostname(), p
	}
	return u.Hostname(), natsClientPort
}

// DNS selects the cluster DNS pods: their namespace and one label. Distributions label CoreDNS
// differently (k8s-app=kube-dns, k8s-app=coredns), so it is the runner's config, not a constant.
type DNS struct{ Namespace, LabelKey, LabelValue string }

var ciliumPolicyKind = schema.GroupVersionKind{Group: "cilium.io", Version: "v2", Kind: "CiliumNetworkPolicy"}

// AnthropicHosts are the hosts a lane's Claude Code reaches directly in plan sign-in mode (ADR 0046
// amendment), from https://code.claude.com/docs/en/network-config ("Network access requirements"):
// the API, claude.ai account authentication, and the OAuth token exchange and refresh.
var AnthropicHosts = []string{"api.anthropic.com", "claude.ai", "platform.claude.com"}

// Namespace is a session's lane namespace: rl-<ticket key, lowercased>, one live session per ticket.
func Namespace(s *Session) string { return "rl-" + strings.ToLower(s.Spec.Ticket.Key) }

func laneLabels(name, id string) map[string]string {
	return map[string]string{
		"app.kubernetes.io/name":       name,
		"app.kubernetes.io/component":  "lane",
		"app.kubernetes.io/part-of":    "rangelight",
		"app.kubernetes.io/managed-by": "rangelight-runner",
		LabelSession:                   id,
	}
}

func objectMeta(name, namespace, id string) metav1.ObjectMeta {
	return metav1.ObjectMeta{Name: name, Namespace: namespace, Labels: laneLabels(name, id)}
}

func namespace(s *Session) *corev1.Namespace {
	ns := &corev1.Namespace{ObjectMeta: objectMeta(Namespace(s), "", s.Spec.ID)}
	ns.Annotations = map[string]string{AnnotationSessionUID: string(s.UID)}
	ns.Labels["pod-security.kubernetes.io/enforce"] = "restricted"
	ns.Labels[LabelLane] = "true"
	ns.Labels[LabelClaudeHome] = "true"
	return ns
}

// egressPolicy denies all ingress, and all egress but the git mirror and NATS's client port. Nothing
// allows the API server (threat model §7 #16). DNS isn't here: an L4 allow to kube-dns would let
// Cilium answer any name, so DNS and Anthropic come only from ciliumPolicy. At NATS the lane signs
// in as its own terminal and may use nothing else (ADR 0045 amendment).
func egressPolicy(s *Session, natsPort int) *networkingv1.NetworkPolicy {
	return &networkingv1.NetworkPolicy{
		ObjectMeta: objectMeta(laneName, Namespace(s), s.Spec.ID),
		Spec: networkingv1.NetworkPolicySpec{
			PolicyTypes: []networkingv1.PolicyType{networkingv1.PolicyTypeIngress, networkingv1.PolicyTypeEgress},
			Egress:      []networkingv1.NetworkPolicyEgressRule{toSystemApp(mirrorApp, mirrorPort), toSystemApp(natsApp, natsPort)},
		},
	}
}

// toSystemApp allows one app's pods in rangelight-system, on one TCP port.
func toSystemApp(app string, port int) networkingv1.NetworkPolicyEgressRule {
	tcp, p := corev1.ProtocolTCP, intstr.FromInt(port)
	return networkingv1.NetworkPolicyEgressRule{
		To: []networkingv1.NetworkPolicyPeer{{
			NamespaceSelector: &metav1.LabelSelector{MatchLabels: map[string]string{"kubernetes.io/metadata.name": systemNamespace}},
			PodSelector:       &metav1.LabelSelector{MatchLabels: map[string]string{"app.kubernetes.io/name": app}},
		}},
		Ports: []networkingv1.NetworkPolicyPort{{Protocol: &tcp, Port: &p}},
	}
}

// ciliumPolicy is the lane's DNS, its Anthropic egress (ADR 0046 amendment) and its egress to an
// external NATS. Cilium's DNS proxy answers only Anthropic's hosts and the mirror's and NATS's names,
// so query names can't carry data out, and toFQDNs learns the addresses from those answers. The
// bundled NATS is a Service in the cluster, which egressPolicy allows by its pods.
func ciliumPolicy(s *Session, mirrorHost, natsHost string, natsPort int, dns DNS) *unstructured.Unstructured {
	fqdns := make([]any, 0, len(AnthropicHosts))
	names := []any{map[string]any{"matchName": mirrorHost}, map[string]any{"matchName": natsHost}}
	for _, h := range AnthropicHosts {
		fqdns = append(fqdns, map[string]any{"matchName": h})
		names = append(names, map[string]any{"matchName": h})
	}
	p := &unstructured.Unstructured{Object: map[string]any{
		"spec": map[string]any{
			"endpointSelector": map[string]any{},
			"egress": []any{
				map[string]any{
					"toEndpoints": []any{map[string]any{"matchLabels": map[string]any{
						"k8s:io.kubernetes.pod.namespace": dns.Namespace, "k8s:" + dns.LabelKey: dns.LabelValue,
					}}},
					"toPorts": []any{map[string]any{
						"ports": []any{map[string]any{"port": "53", "protocol": "ANY"}},
						"rules": map[string]any{"dns": names},
					}},
				},
				map[string]any{
					"toFQDNs": fqdns,
					"toPorts": []any{map[string]any{"ports": []any{map[string]any{"port": "443", "protocol": "TCP"}}}},
				},
				map[string]any{
					"toFQDNs": []any{map[string]any{"matchName": natsHost}},
					"toPorts": []any{map[string]any{"ports": []any{map[string]any{"port": strconv.Itoa(natsPort), "protocol": "TCP"}}}},
				},
			},
		},
	}}
	p.SetGroupVersionKind(ciliumPolicyKind)
	p.SetName(laneName)
	p.SetNamespace(Namespace(s))
	p.SetLabels(laneLabels(laneName, s.Spec.ID))
	return p
}

// claudeHomeClaim is the member's Claude home. The StorageClass maps every claim labelled with the
// same member to one folder (ADR 0046 amendment); the runner never reads it.
func claudeHomeClaim(s *Session) *corev1.PersistentVolumeClaim {
	class := claudeHomeClass
	c := &corev1.PersistentVolumeClaim{
		ObjectMeta: objectMeta(claudeHome, Namespace(s), s.Spec.ID),
		Spec: corev1.PersistentVolumeClaimSpec{
			StorageClassName: &class,
			AccessModes:      []corev1.PersistentVolumeAccessMode{corev1.ReadWriteOnce},
			Resources: corev1.VolumeResourceRequirements{
				Requests: corev1.ResourceList{corev1.ResourceStorage: resource.MustParse("1Gi")},
			},
		},
	}
	c.Labels[LabelMember] = s.Spec.Member
	return c
}

func laneConfig(s *Session, ticket, profile []byte) *corev1.ConfigMap {
	return &corev1.ConfigMap{
		ObjectMeta: objectMeta(laneName, Namespace(s), s.Spec.ID),
		Data:       map[string]string{"ticket.json": string(ticket), "profile.json": string(profile)},
	}
}
