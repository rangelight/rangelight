package session

import (
	"context"
	"errors"
	"testing"

	"k8s.io/apimachinery/pkg/api/meta"
	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
	"k8s.io/apimachinery/pkg/runtime/schema"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/client/fake"
	"sigs.k8s.io/controller-runtime/pkg/client/interceptor"
)

// A cluster without Cilium can't give a lane its Anthropic egress: the session fails and says why,
// instead of starting a lane that can't reach Claude.
func TestFailsWithoutCilium(t *testing.T) {
	c := fake.NewClientBuilder().WithInterceptorFuncs(interceptor.Funcs{
		Get: func(ctx context.Context, c client.WithWatch, key client.ObjectKey, obj client.Object, opts ...client.GetOption) error {
			if _, ok := obj.(*unstructured.Unstructured); ok {
				return &meta.NoKindMatchError{GroupKind: schema.GroupKind{Group: "cilium.io", Kind: "CiliumNetworkPolicy"}}
			}
			return c.Get(ctx, key, obj, opts...)
		},
	}).Build()
	s := &Session{Spec: Spec{ID: "019a5e1c-0000-7000-8000-000000000000", Ticket: Ticket{Key: "RAN-1"}}}
	err := (&Reconciler{Client: c}).ensurePolicies(context.Background(), s)
	var f *failedError
	if !errors.As(err, &f) || f.msg != "CiliumNetworkPolicy is not installed: a lane reaches Anthropic only through Cilium (ADR 0046)" {
		t.Fatalf("got %v, want a failedError", err)
	}
}
