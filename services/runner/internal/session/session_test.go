package session

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"reflect"
	"slices"
	"strings"
	"testing"

	corev1 "k8s.io/api/core/v1"
	networkingv1 "k8s.io/api/networking/v1"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/yaml"

	runnerv1 "github.com/rangelight/rangelight/shared/go/gen/rangelight/runner/v1alpha1"
)

var nextID = 0

// newSession creates a Session for a fresh ticket key on repo, with a valid profile on main unless
// the repo already exists.
func newSession(t *testing.T, repo string, mutate ...func(*Session)) *Session {
	t.Helper()
	nextID++
	s := &Session{
		ObjectMeta: metav1.ObjectMeta{Name: fmt.Sprintf("s%d", nextID), Namespace: runnerNS},
		Spec: Spec{
			ID:      fmt.Sprintf("019a5e1c-0000-7000-8000-%012d", nextID),
			Ticket:  Ticket{Key: fmt.Sprintf("RAN-%d", 1000+nextID), Title: "A title", Description: "", Criteria: []string{"it works"}},
			Repo:    repo,
			BaseRef: "main",
			Member:  "owner",
		},
	}
	for _, f := range mutate {
		f(s)
	}
	if err := k8s.Create(context.Background(), s); err != nil {
		t.Fatal(err)
	}
	return s
}

func validProfile(t *testing.T) string {
	t.Helper()
	raw, err := os.ReadFile("../../../../api/examples/rangelight.otel-demo.example.yaml")
	if err != nil {
		t.Fatal(err)
	}
	return string(raw)
}

func demoRepo(t *testing.T, name string) {
	t.Helper()
	mirror.repo(t, name, map[string]map[string]string{"main": {"rangelight.yaml": validProfile(t)}})
}

// get waits until obj (with its name and namespace set) exists, and reads it.
func get(t *testing.T, obj client.Object) {
	t.Helper()
	key := client.ObjectKeyFromObject(obj)
	eventually(t, fmt.Sprintf("%T %s", obj, key), func() error { return k8s.Get(context.Background(), key, obj) })
}

// gone waits until the Session is deleted, which needs the runner to let go of its finalizer.
func gone(t *testing.T, s *Session) {
	t.Helper()
	eventually(t, "session gone", func() error {
		if err := k8s.Get(context.Background(), client.ObjectKeyFromObject(s), &Session{}); !apierrors.IsNotFound(err) {
			return fmt.Errorf("still there: %w", err)
		}
		return nil
	})
}

func phase(t *testing.T, s *Session, want Phase) *Session {
	t.Helper()
	var got Session
	eventually(t, "phase "+string(want), func() error {
		if err := k8s.Get(context.Background(), client.ObjectKeyFromObject(s), &got); err != nil {
			return err
		}
		if got.Status.Phase != want {
			return fmt.Errorf("phase %q (%s)", got.Status.Phase, got.Status.Message)
		}
		return nil
	})
	return &got
}

func TestCreatesRestrictedNamespace(t *testing.T) {
	demoRepo(t, "ac1")
	s := newSession(t, "ac1")
	ns := &corev1.Namespace{ObjectMeta: metav1.ObjectMeta{Name: "rl-" + strings.ToLower(s.Spec.Ticket.Key)}}
	get(t, ns)
	want := map[string]string{
		"pod-security.kubernetes.io/enforce": "restricted",
		"rangelight.dev/claude-home":         "true",
		"rangelight.dev/lane":                "true",
		"rangelight.dev/session":             s.Spec.ID,
		"app.kubernetes.io/part-of":          "rangelight",
	}
	for k, v := range want {
		if ns.Labels[k] != v {
			t.Errorf("label %s = %q, want %q", k, ns.Labels[k], v)
		}
	}
	phase(t, s, PhaseStarting)
}

func TestDefaultDenyEgress(t *testing.T) {
	demoRepo(t, "ac2")
	s := newSession(t, "ac2")
	np := &networkingv1.NetworkPolicy{ObjectMeta: metav1.ObjectMeta{Name: "lane", Namespace: Namespace(s)}}
	get(t, np)
	if len(np.Spec.PodSelector.MatchLabels)+len(np.Spec.PodSelector.MatchExpressions) != 0 {
		t.Errorf("the policy selects %v, not every pod", np.Spec.PodSelector)
	}
	if !reflect.DeepEqual(np.Spec.PolicyTypes, []networkingv1.PolicyType{"Ingress", "Egress"}) || len(np.Spec.Ingress) != 0 {
		t.Errorf("not default-deny: types %v, ingress %v", np.Spec.PolicyTypes, np.Spec.Ingress)
	}
	// DNS is only in the Cilium policy (TestAnthropicEgress): an L4 allow here would let any name resolve.
	want := []string{"rangelight-system/app.kubernetes.io/name=git-mirror TCP/8080", "rangelight-system/app.kubernetes.io/name=nats TCP/4222"}
	if got := egressRules(np); !reflect.DeepEqual(got, want) {
		t.Errorf("egress = %q, want %q", got, want)
	}
	t.Run("t16_agent_cannot_reach_apiserver", func(t *testing.T) { noRuleReachesTheAPIServer(t, np) })
}

// noRuleReachesTheAPIServer: every egress peer is a pod in a named namespace. The API server is
// never a pod the policy can select, so only an ipBlock, or a rule without peers, could allow it.
func noRuleReachesTheAPIServer(t *testing.T, np *networkingv1.NetworkPolicy) {
	t.Helper()
	for _, rule := range np.Spec.Egress {
		if len(rule.To) == 0 || len(rule.Ports) == 0 {
			t.Errorf("a rule open to every peer or port: %+v", rule)
		}
		for _, peer := range rule.To {
			if peer.IPBlock != nil || peer.NamespaceSelector == nil || peer.PodSelector == nil {
				t.Errorf("a peer that could be the API server: %+v", peer)
			}
		}
	}
}

// egressRules renders each egress rule as "<namespace>/<pod label>=<value> <proto>/<port>...".
func egressRules(np *networkingv1.NetworkPolicy) []string {
	var out []string
	for _, rule := range np.Spec.Egress {
		var parts []string
		for _, peer := range rule.To {
			if peer.NamespaceSelector == nil || peer.PodSelector == nil {
				parts = append(parts, fmt.Sprintf("%+v", peer))
				continue
			}
			for k, v := range peer.PodSelector.MatchLabels {
				parts = append(parts, fmt.Sprintf("%s/%s=%s", peer.NamespaceSelector.MatchLabels["kubernetes.io/metadata.name"], k, v))
			}
		}
		for _, p := range rule.Ports {
			parts = append(parts, fmt.Sprintf("%s/%s", *p.Protocol, p.Port))
		}
		out = append(out, strings.Join(parts, " "))
	}
	return out
}

func TestAgentPodHardened(t *testing.T) {
	demoRepo(t, "ac3")
	s := newSession(t, "ac3")
	pod := &corev1.Pod{ObjectMeta: metav1.ObjectMeta{Name: "agent", Namespace: Namespace(s)}}
	ensureServiceAccount(t, Namespace(s))
	get(t, pod)
	ps, c := pod.Spec.SecurityContext, pod.Spec.Containers[0]
	checks := []struct {
		name string
		ok   bool
	}{
		{"one container", len(pod.Spec.Containers) == 1 && len(pod.Spec.InitContainers) == 0},
		{"the agent image", c.Image == testImage},
		{"non-root", *ps.RunAsNonRoot && *ps.RunAsUser == 1000 && *c.SecurityContext.RunAsNonRoot},
		{"read-only root filesystem", *c.SecurityContext.ReadOnlyRootFilesystem},
		{"no privilege escalation", !*c.SecurityContext.AllowPrivilegeEscalation},
		{"capabilities drop ALL", reflect.DeepEqual(c.SecurityContext.Capabilities.Drop, []corev1.Capability{"ALL"}) && len(c.SecurityContext.Capabilities.Add) == 0},
		{"seccomp RuntimeDefault", ps.SeccompProfile.Type == corev1.SeccompProfileTypeRuntimeDefault},
		{"no automounted token", !*pod.Spec.AutomountServiceAccountToken},
		{"resources", !c.Resources.Requests.Cpu().IsZero() && !c.Resources.Limits.Memory().IsZero()},
		{"two projected tokens, the mirror's and NATS's", projectedTokens(pod) == "rangelight-git-mirror/600,rangelight-nats/3600"},
		{"tokens mounted where the agent reads them", mounted(c, "git-mirror-token", "/var/run/secrets/rangelight/git-mirror", true) &&
			mounted(c, "nats-token", "/var/run/secrets/rangelight/nats", true)},
		{"ticket and profile from the lane ConfigMap", mounted(c, "lane", "/etc/rangelight", true)},
		{"worktree on an emptyDir", mounted(c, "workspace", "/workspace", false) && volume(pod, "workspace").EmptyDir != nil},
		{"bounded disk", *volume(pod, "workspace").EmptyDir.SizeLimit == options.Workspace &&
			*volume(pod, "tmp").EmptyDir.SizeLimit == options.Tmp &&
			c.Resources.Limits.StorageEphemeral().Equal(options.EphemeralStorage)},
		{"names looked up as given (ndots 1)", pod.Spec.DNSConfig != nil && len(pod.Spec.DNSConfig.Options) == 1 &&
			pod.Spec.DNSConfig.Options[0].Name == "ndots" && *pod.Spec.DNSConfig.Options[0].Value == "1"},
		{"agent settings", env(c, "RANGELIGHT_AGENT_SESSION_ID") == s.Spec.ID && env(c, "RANGELIGHT_AGENT_REPO") == "ac3" &&
			env(c, "RANGELIGHT_AGENT_BASE_REF") == "main" && env(c, "RANGELIGHT_AGENT_MIRROR_URL") == mirror.URL &&
			env(c, "RANGELIGHT_AGENT_NATS_URL") == options.NATSURL && env(c, "RANGELIGHT_AGENT_ORG") == "acme"},
	}
	for _, check := range checks {
		if !check.ok {
			t.Errorf("agent pod: not %s", check.name)
		}
	}
}

// projectedTokens lists the pod's projected sources as audience/expiry, each a ServiceAccount
// token at the path token.
func projectedTokens(pod *corev1.Pod) string {
	var tokens []string
	for _, v := range pod.Spec.Volumes {
		if v.Projected != nil {
			for _, src := range v.Projected.Sources {
				t := src.ServiceAccountToken
				if t == nil || t.Path != "token" || t.ExpirationSeconds == nil {
					return "not only tokens"
				}
				tokens = append(tokens, fmt.Sprintf("%s/%d", t.Audience, *t.ExpirationSeconds))
			}
		}
	}
	return strings.Join(tokens, ",")
}

func mounted(c corev1.Container, name, path string, readOnly bool) bool {
	return slices.ContainsFunc(c.VolumeMounts, func(m corev1.VolumeMount) bool {
		return m.Name == name && m.MountPath == path && m.ReadOnly == readOnly
	})
}

func volume(pod *corev1.Pod, name string) corev1.Volume {
	i := slices.IndexFunc(pod.Spec.Volumes, func(v corev1.Volume) bool { return v.Name == name })
	if i < 0 {
		return corev1.Volume{}
	}
	return pod.Spec.Volumes[i]
}

func env(c corev1.Container, name string) string {
	i := slices.IndexFunc(c.Env, func(e corev1.EnvVar) bool { return e.Name == name })
	if i < 0 {
		return ""
	}
	return c.Env[i].Value
}

// setAgentStatus sets the agent pod's status as the kubelet would: the pod Ready with its agent
// container, the container's state, and its last termination.
func setAgentStatus(t *testing.T, pod *corev1.Pod, ready bool, state corev1.ContainerState, last *corev1.ContainerStateTerminated, restarts ...int32) {
	t.Helper()
	condition := corev1.ConditionFalse
	if ready {
		condition = corev1.ConditionTrue
	}
	pod.Status.Conditions = []corev1.PodCondition{{Type: corev1.PodReady, Status: condition}}
	pod.Status.ContainerStatuses = []corev1.ContainerStatus{{
		Name: agentName, Ready: ready, State: state, Image: testImage, ImageID: testImage,
		LastTerminationState: corev1.ContainerState{Terminated: last},
	}}
	if len(restarts) > 0 {
		pod.Status.ContainerStatuses[0].RestartCount = restarts[0]
	}
	if err := k8s.Status().Update(context.Background(), pod); err != nil {
		t.Fatal(err)
	}
}

// RAN-35 AC2 (live check, review M1): an agent container that can't start (ImagePullBackOff) stays
// Starting with its reason in status.message, reported; one that crash-loops crashLoops times isn't
// Hot: the Session fails with the last exit's reason and code, and reports FAILED.
func TestStuckOrCrashLoopingAgent(t *testing.T) {
	demoRepo(t, "crash")
	s := newSession(t, "crash")
	ensureServiceAccount(t, Namespace(s))
	pod := &corev1.Pod{ObjectMeta: metav1.ObjectMeta{Name: agentName, Namespace: Namespace(s)}}
	get(t, pod)
	// The pod's Ready condition alone (what phase used to follow) doesn't make it Hot.
	pod.Status.Conditions = []corev1.PodCondition{{Type: corev1.PodReady, Status: corev1.ConditionTrue}}
	if err := k8s.Status().Update(context.Background(), pod); err != nil {
		t.Fatal(err)
	}
	phase(t, s, PhaseStarting)
	// A container that can't start yet stays Starting, with the reason the Workbench shows, reported.
	setAgentStatus(t, pod, false,
		corev1.ContainerState{Waiting: &corev1.ContainerStateWaiting{Reason: "ImagePullBackOff", Message: "Back-off pulling image"}}, nil)
	const pulling = "agent container waiting: ImagePullBackOff: Back-off pulling image"
	eventually(t, "waiting reason", func() error {
		var got Session
		if err := k8s.Get(context.Background(), client.ObjectKeyFromObject(s), &got); err != nil {
			return err
		}
		if got.Status.Phase != PhaseStarting || got.Status.Message != pulling {
			return fmt.Errorf("%s %q", got.Status.Phase, got.Status.Message)
		}
		return nil
	})
	eventually(t, "waiting reason reported", func() error {
		found, err := statuses(s.Spec.ID, runnerv1.Phase_PHASE_STARTING)
		if err == nil && !slices.ContainsFunc(found, func(e *runnerv1.Envelope) bool { return e.GetStatus().GetMessage() == pulling }) {
			err = errors.New("not reported")
		}
		return err
	})
	setAgentStatus(t, pod, false,
		corev1.ContainerState{Waiting: &corev1.ContainerStateWaiting{Reason: "CrashLoopBackOff", Message: "back-off 40s"}},
		&corev1.ContainerStateTerminated{Reason: "Error", ExitCode: 3}, crashLoops)
	failed := phase(t, s, PhaseFailed)
	if want := "agent container is crash-looping after 3 restarts: Error (exit code 3)"; !strings.Contains(failed.Status.Message, want) {
		t.Errorf("message %q, want it to contain %q", failed.Status.Message, want)
	}
	if got := reported(t, s.Spec.ID, runnerv1.Phase_PHASE_FAILED); got.GetStatus().GetMessage() != failed.Status.Message {
		t.Errorf("FAILED reported %q, want %q", got.GetStatus().GetMessage(), failed.Status.Message)
	}
}

func TestHotWhenAgentPodReady(t *testing.T) {
	demoRepo(t, "hot")
	s := newSession(t, "hot")
	ensureServiceAccount(t, Namespace(s))
	pod := &corev1.Pod{ObjectMeta: metav1.ObjectMeta{Name: "agent", Namespace: Namespace(s)}}
	get(t, pod)
	phase(t, s, PhaseStarting)
	setAgentStatus(t, pod, true, corev1.ContainerState{Running: &corev1.ContainerStateRunning{}}, nil)
	phase(t, s, PhaseHot)
}

func TestProfileFromBaseBranch(t *testing.T) {
	heads := mirror.repo(t, "ac4", map[string]map[string]string{
		"main":                    {"rangelight.yaml": validProfile(t)},
		"rangelight/session/work": {"rangelight.yaml": "apiVersion: \"rangelight.dev/v1alpha1\"\nkind: \"Other\"\n"},
	})
	s := newSession(t, "ac4")
	cm := &corev1.ConfigMap{ObjectMeta: metav1.ObjectMeta{Name: "lane", Namespace: Namespace(s)}}
	get(t, cm)
	var got, want any
	wantJSON, err := yaml.YAMLToJSON([]byte(validProfile(t)))
	if err != nil {
		t.Fatal(err)
	}
	if err := errors.Join(json.Unmarshal([]byte(cm.Data["profile.json"]), &got), json.Unmarshal(wantJSON, &want)); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("profile.json is not main's rangelight.yaml:\n%s", cm.Data["profile.json"])
	}
	if s := phase(t, s, PhaseStarting); s.Status.ProfileCommit != heads["main"] {
		t.Errorf("profileCommit = %q, want main's %q", s.Status.ProfileCommit, heads["main"])
	}
	var ticket map[string]any
	if err := json.Unmarshal([]byte(cm.Data["ticket.json"]), &ticket); err != nil {
		t.Fatal(err)
	}
	if want := map[string]any{"key": s.Spec.Ticket.Key, "title": "A title", "description": "", "criteria": []any{"it works"}}; !reflect.DeepEqual(ticket, want) {
		t.Errorf("ticket.json = %s, want %v", cm.Data["ticket.json"], want)
	}
}

func TestInvalidProfileFails(t *testing.T) {
	tests := []struct{ name, files, want string }{
		{"schema error", "apiVersion: \"rangelight.dev/v1alpha1\"\nkind: \"PreviewProfile\"\nmetadata: {name: \"x\"}\nspec: {}\n", "missing property 'services'"},
		{"not YAML", "spec: [\n", "rangelight.yaml: yaml: line 1"},
		{"no profile", "", "rangelight.yaml: not found at"},
	}
	for i, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			files := map[string]string{"README.md": "x"}
			if tt.files != "" {
				files["rangelight.yaml"] = tt.files
			}
			repo := fmt.Sprintf("bad%d", i)
			mirror.repo(t, repo, map[string]map[string]string{"main": files})
			s := phase(t, newSession(t, repo), PhaseFailed)
			if !strings.Contains(s.Status.Message, tt.want) {
				t.Errorf("message = %q, want it to contain %q", s.Status.Message, tt.want)
			}
			if err := k8s.Get(context.Background(), client.ObjectKey{Name: Namespace(s)}, &corev1.Namespace{}); !apierrors.IsNotFound(err) {
				t.Errorf("a failed profile still made the namespace: %v", err)
			}
		})
	}
}

func TestDeleteCleansUp(t *testing.T) {
	demoRepo(t, "ac5")
	s := newSession(t, "ac5")
	ns := &corev1.Namespace{ObjectMeta: metav1.ObjectMeta{Name: Namespace(s)}}
	get(t, ns)
	phase(t, s, PhaseStarting)
	if err := k8s.Delete(context.Background(), s); err != nil {
		t.Fatal(err)
	}
	gone(t, s)
	// envtest runs no namespace controller, so the namespace stays Terminating instead of going away.
	eventually(t, "namespace deleted", func() error {
		err := k8s.Get(context.Background(), client.ObjectKeyFromObject(ns), ns)
		if apierrors.IsNotFound(err) || (err == nil && !ns.DeletionTimestamp.IsZero()) {
			return nil
		}
		return fmt.Errorf("not deleted: %w", err)
	})
}

// A namespace made in advance, even one carrying the session's label, is never adopted: the session
// fails and nothing is created in it (RAN-43 review: it would capture the lane's NATS identity).
func TestNeverAdoptsAForeignNamespace(t *testing.T) {
	demoRepo(t, "foreign")
	forged := map[string]string{LabelSession: fmt.Sprintf("019a5e1c-0000-7000-8000-%012d", nextID+1)}
	ns := &corev1.Namespace{ObjectMeta: metav1.ObjectMeta{Name: fmt.Sprintf("rl-ran-%d", 1000+nextID+1), Labels: forged}}
	if err := k8s.Create(context.Background(), ns); err != nil {
		t.Fatal(err)
	}
	s := phase(t, newSession(t, "foreign"), PhaseFailed)
	var pods corev1.PodList
	if err := k8s.List(context.Background(), &pods, client.InNamespace(ns.Name)); err != nil || len(pods.Items) != 0 {
		t.Errorf("created %d pods in a namespace that wasn't its own: %v", len(pods.Items), err)
	}
	if want := "namespace_taken: namespace " + ns.Name + " already exists and is not this session's"; s.Status.Message != want {
		t.Errorf("message = %q, want %q", s.Status.Message, want)
	}
	if err := k8s.Delete(context.Background(), s); err != nil {
		t.Fatal(err)
	}
	gone(t, s)
	if err := k8s.Get(context.Background(), client.ObjectKeyFromObject(ns), ns); err != nil || !ns.DeletionTimestamp.IsZero() {
		t.Errorf("deleting the session touched a namespace that wasn't its own: %v", err)
	}
}

func TestClaudeHome(t *testing.T) {
	demoRepo(t, "ac7")
	s := newSession(t, "ac7", func(s *Session) { s.Spec.Member = "ariel" })
	pvc := &corev1.PersistentVolumeClaim{ObjectMeta: metav1.ObjectMeta{Name: "claude-home", Namespace: Namespace(s)}}
	get(t, pvc)
	if pvc.Labels["rangelight.dev/member"] != "ariel" || *pvc.Spec.StorageClassName != "rangelight-claude-home" {
		t.Errorf("claim: member %q, class %q", pvc.Labels["rangelight.dev/member"], *pvc.Spec.StorageClassName)
	}
	ensureServiceAccount(t, Namespace(s))
	pod := &corev1.Pod{ObjectMeta: metav1.ObjectMeta{Name: "agent", Namespace: Namespace(s)}}
	get(t, pod)
	c := pod.Spec.Containers[0]
	dir := env(c, "CLAUDE_CONFIG_DIR")
	if dir == "" || !mounted(c, "claude-home", dir, false) || volume(pod, "claude-home").PersistentVolumeClaim.ClaimName != "claude-home" {
		t.Errorf("the claim is not mounted as CLAUDE_CONFIG_DIR (%q)", dir)
	}
}

func TestRejectsAnInvalidSpec(t *testing.T) {
	tests := []struct {
		name   string
		mutate func(*Session)
	}{
		{"member with capitals", func(s *Session) { s.Spec.Member = "Ariel" }},
		{"member too long", func(s *Session) { s.Spec.Member = strings.Repeat("a", 64) }},
		{"member ending in -", func(s *Session) { s.Spec.Member = "ariel-" }},
		{"id not a UUIDv7", func(s *Session) { s.Spec.ID = "019a5e1c-0000-4000-8000-000000000000" }},
		{"ticket key lowercase", func(s *Session) { s.Spec.Ticket.Key = "ran-1" }},
		{"no criteria", func(s *Session) { s.Spec.Ticket.Criteria = nil }},
		{"repo with .git", func(s *Session) { s.Spec.Repo = "demo.git" }},
		{"repo with a slash", func(s *Session) { s.Spec.Repo = "org/demo" }},
		{"baseRef with ..", func(s *Session) { s.Spec.BaseRef = "main/../x" }},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := &Session{
				ObjectMeta: metav1.ObjectMeta{Name: "invalid", Namespace: runnerNS},
				Spec: Spec{ID: "019a5e1c-0000-7000-8000-999999999999", Repo: "demo", BaseRef: "main", Member: "owner",
					Ticket: Ticket{Key: "RAN-1", Title: "t", Criteria: []string{"c"}}},
			}
			tt.mutate(s)
			if err := k8s.Create(context.Background(), s); !apierrors.IsInvalid(err) {
				t.Errorf("created, or not refused as invalid: %v", err)
			}
		})
	}
}

// Review: the CRD's workspace rules are rl_domain::Workspace's: both check test/workspace.cases.json.
func TestWorkspaceRulesAreTheSharedCases(t *testing.T) {
	raw, err := os.ReadFile("../../../../test/workspace.cases.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Repo, BaseRef, Member string
		Valid                 bool
	}
	if err := json.Unmarshal(raw, &cases); err != nil {
		t.Fatal(err)
	}
	for _, c := range cases {
		s := &Session{
			ObjectMeta: metav1.ObjectMeta{Name: "workspace-case", Namespace: runnerNS},
			Spec: Spec{ID: "019a5e1c-0000-7000-8000-999999999998", Repo: c.Repo, BaseRef: c.BaseRef, Member: c.Member,
				Ticket: Ticket{Key: "RAN-1", Title: "t", Criteria: []string{"c"}}},
		}
		err := k8s.Create(context.Background(), s, client.DryRunAll)
		if (err == nil) != c.Valid || (err != nil && !apierrors.IsInvalid(err)) {
			t.Errorf("%+v: the API server says %v", c, err)
		}
	}
}

func TestSpecIdentityIsImmutable(t *testing.T) {
	demoRepo(t, "immutable")
	s := newSession(t, "immutable")
	for name, mutate := range map[string]func(*Session){
		"member": func(s *Session) { s.Spec.Member = "someone" },
		"repo":   func(s *Session) { s.Spec.Repo = "other" },
		"key":    func(s *Session) { s.Spec.Ticket.Key = "RAN-9" },
	} {
		// The runner updates the Session too: retry on a conflict until the API server decides.
		eventually(t, name+" is immutable", func() error {
			var got Session
			if err := k8s.Get(context.Background(), client.ObjectKeyFromObject(s), &got); err != nil {
				return err
			}
			mutate(&got)
			if err := k8s.Update(context.Background(), &got); !apierrors.IsInvalid(err) {
				return fmt.Errorf("%s changed: %w", name, err)
			}
			return nil
		})
	}
}

func TestAnthropicEgress(t *testing.T) {
	demoRepo(t, "ac8")
	s := newSession(t, "ac8")
	cnp := &unstructured.Unstructured{}
	cnp.SetAPIVersion("cilium.io/v2")
	cnp.SetKind("CiliumNetworkPolicy")
	cnp.SetName("lane")
	cnp.SetNamespace(Namespace(s))
	get(t, cnp)
	var spec struct {
		EndpointSelector map[string]any `json:"endpointSelector"`
		Egress           []struct {
			ToEndpoints []struct {
				MatchLabels map[string]string `json:"matchLabels"`
			} `json:"toEndpoints"`
			ToFQDNs []struct {
				MatchName string `json:"matchName"`
			} `json:"toFQDNs"`
			ToPorts []struct {
				Ports []struct{ Port, Protocol string } `json:"ports"`
				Rules struct {
					DNS []struct {
						MatchName    string `json:"matchName"`
						MatchPattern string `json:"matchPattern"`
					} `json:"dns"`
				} `json:"rules"`
			} `json:"toPorts"`
		} `json:"egress"`
	}
	raw, _ := json.Marshal(cnp.Object["spec"])
	if err := json.Unmarshal(raw, &spec); err != nil || len(spec.Egress) != 3 || len(spec.EndpointSelector) != 0 {
		t.Fatalf("spec %s: %v", raw, err)
	}
	dns, web, nats := spec.Egress[0], spec.Egress[1], spec.Egress[2]
	if dns.ToEndpoints[0].MatchLabels["k8s:k8s-app"] != "coredns" || dns.ToEndpoints[0].MatchLabels["k8s:io.kubernetes.pod.namespace"] != "kube-system" ||
		dns.ToPorts[0].Ports[0].Port != "53" {
		t.Errorf("DNS does not go through Cilium's DNS proxy: %s", raw)
	}
	// Only exact names resolve, so a query name can't carry data out.
	var names []string
	for _, rule := range dns.ToPorts[0].Rules.DNS {
		names = append(names, rule.MatchName+rule.MatchPattern)
	}
	if want := []string{strings.Split(strings.TrimPrefix(mirror.URL, "http://"), ":")[0], natsHost, "api.anthropic.com", "claude.ai", "platform.claude.com"}; !reflect.DeepEqual(names, want) {
		t.Errorf("DNS answers %q, want only %q", names, want)
	}
	var hosts []string
	for _, f := range web.ToFQDNs {
		hosts = append(hosts, f.MatchName)
	}
	if !reflect.DeepEqual(hosts, []string{"api.anthropic.com", "claude.ai", "platform.claude.com"}) ||
		len(web.ToPorts) != 1 || web.ToPorts[0].Ports[0] != (struct{ Port, Protocol string }{"443", "TCP"}) {
		t.Errorf("Anthropic egress: %s", raw)
	}
	// An external NATS, on its client port only (RAN-41); the bundled one is TestDefaultDenyEgress's.
	if len(nats.ToFQDNs) != 1 || nats.ToFQDNs[0].MatchName != natsHost || len(nats.ToPorts) != 1 ||
		nats.ToPorts[0].Ports[0] != (struct{ Port, Protocol string }{"4222", "TCP"}) {
		t.Errorf("NATS egress: %s", raw)
	}
}

// natsHost is the host of the suite's nats_url.
const natsHost = "rangelight-nats.rangelight-system.svc"

func TestRetriesUntilTheMirrorHasTheRepo(t *testing.T) {
	s := newSession(t, "late")
	phase(t, s, PhasePending)
	demoRepo(t, "late")
	phase(t, s, PhaseStarting)
}

// The runner reads the profile as itself: its own token, as the mirror's Basic password (R1).
func TestProfileIsReadAsTheRunner(t *testing.T) {
	demoRepo(t, "asrunner")
	phase(t, newSession(t, "asrunner"), PhaseStarting)
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	req.Header.Set("Authorization", mirror.authorization("asrunner"))
	if user, pass, ok := req.BasicAuth(); !ok || user != "runner" || pass != runnerToken {
		t.Errorf("the mirror got Authorization %q, want Basic runner:<the runner's token>", mirror.authorization("asrunner"))
	}
}

// A ConfigMap another session of the same ticket left behind is never this session's profile.
func TestReusesOnlyItsOwnProfile(t *testing.T) {
	ctx := context.Background()
	spec := Spec{ID: "019a5e1c-0000-7000-8000-0000000000dd", Ticket: Ticket{Key: "RAN-77777"}}
	// The session waits for its repo; meanwhile its namespace holds the lane ConfigMap an earlier session of
	// the ticket left in it.
	s := newSession(t, "reuse", func(s *Session) { s.Spec.ID, s.Spec.Ticket.Key = spec.ID, spec.Ticket.Key })
	stale := laneConfig(&Session{Spec: Spec{ID: "019a5e1c-0000-7000-8000-0000000000ee", Ticket: spec.Ticket}}, []byte("{}"), []byte(`{"stale":true}`))
	if err := errors.Join(k8s.Create(ctx, namespace(s)), k8s.Create(ctx, stale)); err != nil {
		t.Fatal(err)
	}
	heads := mirror.repo(t, "reuse", map[string]map[string]string{"main": {"rangelight.yaml": validProfile(t)}})
	eventually(t, "the profile read from main", func() error {
		var cm corev1.ConfigMap
		if err := k8s.Get(ctx, client.ObjectKeyFromObject(stale), &cm); err != nil {
			return err
		}
		if strings.Contains(cm.Data["profile.json"], "stale") || cm.Labels[LabelSession] != s.Spec.ID {
			return fmt.Errorf("still the earlier session's: %v", cm.Labels)
		}
		return nil
	})
	if got := phase(t, s, PhaseStarting); got.Status.ProfileCommit != heads["main"] {
		t.Errorf("profileCommit %q, want %q", got.Status.ProfileCommit, heads["main"])
	}
}

// A mirror that never answers holds one worker for the fetch timeout, not every Session (S4).
func TestSlowMirrorDoesNotStallOtherSessions(t *testing.T) {
	slow := newSession(t, "slow")
	demoRepo(t, "fast")
	fast := newSession(t, "fast")
	phase(t, fast, PhaseStarting)
	var got Session
	if err := k8s.Get(context.Background(), client.ObjectKeyFromObject(slow), &got); err != nil {
		t.Fatal(err)
	}
	if got.Status.Phase != "" && got.Status.Phase != PhasePending {
		t.Errorf("the slow session is %s", got.Status.Phase)
	}
}

// A changed lane policy is put back at once: the runner watches them (N1).
func TestTamperedPoliciesArePutBack(t *testing.T) {
	ctx := context.Background()
	demoRepo(t, "tamper")
	s := newSession(t, "tamper")
	np := &networkingv1.NetworkPolicy{ObjectMeta: metav1.ObjectMeta{Name: "lane", Namespace: Namespace(s)}}
	cnp := &unstructured.Unstructured{}
	cnp.SetGroupVersionKind(ciliumPolicyKind)
	cnp.SetName("lane")
	cnp.SetNamespace(Namespace(s))
	get(t, np)
	get(t, cnp)
	np.Spec.Egress = append(np.Spec.Egress, networkingv1.NetworkPolicyEgressRule{}) // allow everything
	cnp.Object["spec"] = map[string]any{"endpointSelector": map[string]any{}, "egress": []any{map[string]any{"toFQDNs": []any{map[string]any{"matchPattern": "*"}}}}}
	if err := errors.Join(k8s.Update(ctx, np), k8s.Update(ctx, cnp)); err != nil {
		t.Fatal(err)
	}
	want := egressPolicy(s, 4222).Spec
	wantCilium := ciliumPolicy(s, strings.Split(strings.TrimPrefix(mirror.URL, "http://"), ":")[0], natsHost, 4222, options.DNS).Object["spec"]
	// Well inside the 10-hour resync: only a watch puts them back this fast.
	eventually(t, "both policies restored", func() error {
		if err := errors.Join(k8s.Get(ctx, client.ObjectKeyFromObject(np), np), k8s.Get(ctx, client.ObjectKeyFromObject(cnp), cnp)); err != nil {
			return err
		}
		got, _ := json.Marshal(cnp.Object["spec"])
		wantJSON, _ := json.Marshal(wantCilium)
		if len(np.Spec.Egress) != len(want.Egress) || string(got) != string(wantJSON) {
			return fmt.Errorf("still tampered: %d egress rules, cilium %s", len(np.Spec.Egress), got)
		}
		return nil
	})
}
