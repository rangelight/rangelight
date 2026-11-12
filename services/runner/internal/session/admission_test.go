package session

import (
	"context"
	"fmt"
	"strings"
	"testing"

	corev1 "k8s.io/api/core/v1"
	networkingv1 "k8s.io/api/networking/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"
)

// The chart's ValidatingAdmissionPolicy confines the runner's ServiceAccount to lanes, although its
// ClusterRole reaches every namespace. Each forbidden write must be refused by the policy itself:
// the runner's RBAC allows every one of them.
func TestAdmissionPolicyConfinesTheRunner(t *testing.T) {
	ctx := context.Background()
	for _, ns := range []*corev1.Namespace{
		{ObjectMeta: metav1.ObjectMeta{Name: "victim"}},
		{ObjectMeta: metav1.ObjectMeta{Name: "rl-admission", Labels: map[string]string{LabelLane: "true"}}},
	} {
		if err := k8s.Create(ctx, ns); err != nil {
			t.Fatal(err)
		}
	}
	victimPolicy := &networkingv1.NetworkPolicy{ObjectMeta: metav1.ObjectMeta{Name: "p", Namespace: "victim"}}
	if err := k8s.Create(ctx, victimPolicy); err != nil {
		t.Fatal(err)
	}
	pod := func(ns, sa string) *corev1.Pod {
		no := false
		return &corev1.Pod{
			ObjectMeta: metav1.ObjectMeta{GenerateName: "p-", Namespace: ns},
			Spec: corev1.PodSpec{ServiceAccountName: sa, AutomountServiceAccountToken: &no,
				Containers: []corev1.Container{{Name: "c", Image: testImage}}},
		}
	}
	configMap := func(ns string) *corev1.ConfigMap {
		return &corev1.ConfigMap{ObjectMeta: metav1.ObjectMeta{GenerateName: "c-", Namespace: ns}}
	}
	namespace := func(name string, labels map[string]string) *corev1.Namespace {
		return &corev1.Namespace{ObjectMeta: metav1.ObjectMeta{Name: name, Labels: labels}}
	}
	lane := map[string]string{LabelLane: "true"}
	denied := []struct {
		name  string
		write func() error
	}{
		{"a pod in kube-system", func() error { return asRunner.Create(ctx, pod("kube-system", "default")) }},
		{"a pod outside a lane", func() error { return asRunner.Create(ctx, pod("victim", "default")) }},
		{"a lane pod with another ServiceAccount", func() error { return asRunner.Create(ctx, pod("rl-admission", "admin")) }},
		{"a ConfigMap outside a lane", func() error { return asRunner.Create(ctx, configMap("victim")) }},
		{"a NetworkPolicy outside a lane", func() error {
			p := victimPolicy.DeepCopy()
			p.Spec.PolicyTypes = []networkingv1.PolicyType{"Egress"}
			return asRunner.Update(ctx, p)
		}},
		{"a namespace not named rl-", func() error { return asRunner.Create(ctx, namespace("evil", lane)) }},
		{"an rl- namespace without the lane label", func() error { return asRunner.Create(ctx, namespace("rl-unlabelled", nil)) }},
		{"deleting a namespace that isn't a lane", func() error { return asRunner.Delete(ctx, namespace("victim", nil)) }},
		{"deleting a Job of its namespace that isn't a publish Job", func() error {
			job := newPublishJob(&Session{ObjectMeta: metav1.ObjectMeta{Namespace: runnerNS}, Spec: Spec{ID: "019a5e1c-0000-7000-8000-00000000aaaa"},
				Status: Status{Pushes: 1}}, options, "file:///x")
			job.Name, job.Labels = "nats-keys", map[string]string{"app.kubernetes.io/component": "nats"}
			job.Spec.Template.Labels = job.Labels
			if err := k8s.Create(ctx, job); client.IgnoreAlreadyExists(err) != nil {
				return err
			}
			return asRunner.Delete(ctx, job)
		}},
	}
	for _, tt := range denied {
		t.Run("denied: "+tt.name, func(t *testing.T) { refusedByThePolicy(t, tt.write) })
	}
	allowed := []struct {
		name  string
		write func() error
	}{
		{"a ConfigMap in a lane", func() error { return asRunner.Create(ctx, configMap("rl-admission")) }},
		{"a lane pod as default", func() error { return asRunner.Create(ctx, pod("rl-admission", "default")) }},
		{"an rl- lane namespace, and deleting it", func() error {
			ns := namespace("rl-allowed", lane)
			if err := asRunner.Create(ctx, ns); err != nil {
				return err
			}
			return asRunner.Delete(ctx, ns)
		}},
	}
	for _, tt := range allowed {
		t.Run("allowed: "+tt.name, func(t *testing.T) {
			if err := tt.write(); err != nil {
				t.Error(err)
			}
		})
	}
	if err := k8s.Get(ctx, client.ObjectKey{Name: "victim"}, &corev1.Namespace{}); err != nil {
		t.Errorf("the victim namespace: %v", err)
	}
}

func refusedByThePolicy(t *testing.T, write func() error) {
	t.Helper()
	// The API server compiles a new policy within a second or two.
	eventually(t, "the policy refuses it", func() error {
		err := write()
		if err == nil || !strings.Contains(err.Error(), "ValidatingAdmissionPolicy 'rangelight-runner'") {
			return fmt.Errorf("not refused by the policy: %w", err)
		}
		return nil
	})
}
