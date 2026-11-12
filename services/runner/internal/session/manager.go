package session

import (
	"context"
	"fmt"
	"net/url"
	"time"

	batchv1 "k8s.io/api/batch/v1"
	corev1 "k8s.io/api/core/v1"
	networkingv1 "k8s.io/api/networking/v1"
	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
	"k8s.io/apimachinery/pkg/labels"
	"k8s.io/apimachinery/pkg/runtime"
	"k8s.io/apimachinery/pkg/selection"
	clientgoscheme "k8s.io/client-go/kubernetes/scheme"
	"k8s.io/client-go/rest"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/cache"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/controller"
	"sigs.k8s.io/controller-runtime/pkg/handler"
	"sigs.k8s.io/controller-runtime/pkg/manager"
	metricsserver "sigs.k8s.io/controller-runtime/pkg/metrics/server"
	"sigs.k8s.io/controller-runtime/pkg/reconcile"
)

// indexID indexes Sessions by spec.id.
const indexID = "spec.id"

// NewManager is the runner's controller manager: one replica, so no leader election, and no metrics
// listener. It watches Sessions only in the runner's own namespace, and caches only lane objects.
func NewManager(cfg *rest.Config, namespace string) (manager.Manager, error) {
	scheme := runtime.NewScheme()
	if err := clientgoscheme.AddToScheme(scheme); err != nil {
		return nil, fmt.Errorf("scheme: %w", err)
	}
	if err := AddToScheme(scheme); err != nil {
		return nil, fmt.Errorf("scheme: %w", err)
	}
	hasSession, err := labels.NewRequirement(LabelSession, selection.Exists, nil)
	if err != nil {
		return nil, fmt.Errorf("selector: %w", err)
	}
	inLane := cache.ByObject{Label: labels.NewSelector().Add(*hasSession)}
	mgr, err := ctrl.NewManager(cfg, ctrl.Options{
		Scheme:  scheme,
		Metrics: metricsserver.Options{BindAddress: "0"},
		Cache: cache.Options{ByObject: map[client.Object]cache.ByObject{
			&Session{}:                      {Namespaces: map[string]cache.Config{namespace: {}}},
			&corev1.Namespace{}:             {Label: labels.SelectorFromSet(labels.Set{LabelLane: "true"})},
			&corev1.Pod{}:                   inLane,
			&corev1.ConfigMap{}:             inLane,
			&corev1.PersistentVolumeClaim{}: inLane,
			&networkingv1.NetworkPolicy{}:   inLane,
			&batchv1.Job{}:                  {Namespaces: map[string]cache.Config{namespace: {}}},
		}},
	})
	if err != nil {
		return nil, fmt.Errorf("manager: %w", err)
	}
	err = mgr.GetFieldIndexer().IndexField(context.Background(), &Session{}, indexID, func(o client.Object) []string {
		if s, ok := o.(*Session); ok {
			return []string{s.Spec.ID}
		}
		return nil
	})
	if err != nil {
		return nil, fmt.Errorf("index: %w", err)
	}
	return mgr, nil
}

// SetupWithManager registers the reconciler: it runs for every Session, and again as soon as any
// object of its lane changes, so a changed policy or a deleted pod is put back at once.
func (r *Reconciler) SetupWithManager(mgr manager.Manager) error {
	u, err := url.Parse(r.MirrorURL)
	if err != nil {
		return fmt.Errorf("mirror url: %w", err)
	}
	r.mirrorHost = u.Hostname()
	if r.profiles, err = newProfiles(r.MirrorURL, r.MirrorTokenFile); err != nil {
		return err
	}
	r.pub = newPublishing()
	r.mirrorGiveUp = 10 * time.Minute
	toSession := handler.EnqueueRequestsFromMapFunc(func(ctx context.Context, o client.Object) []reconcile.Request {
		s, err := byID(ctx, r.Client, o.GetLabels()[LabelSession])
		if err != nil || s == nil {
			return nil
		}
		return []reconcile.Request{{NamespacedName: client.ObjectKeyFromObject(s)}}
	})
	b := ctrl.NewControllerManagedBy(mgr).For(&Session{}).
		WithOptions(controller.Options{MaxConcurrentReconciles: r.Workers}).
		Owns(&batchv1.Job{}).
		Watches(&corev1.Pod{}, toSession).
		Watches(&corev1.ConfigMap{}, toSession).
		Watches(&corev1.PersistentVolumeClaim{}, toSession).
		Watches(&networkingv1.NetworkPolicy{}, toSession)
	// Without Cilium there is nothing to watch: sessions fail instead (ensurePolicies).
	cnp := &unstructured.Unstructured{}
	cnp.SetGroupVersionKind(ciliumPolicyKind)
	if _, err := mgr.GetRESTMapper().RESTMapping(ciliumPolicyKind.GroupKind(), ciliumPolicyKind.Version); err == nil {
		b = b.Watches(cnp, toSession)
	}
	if err := b.Complete(r); err != nil {
		return fmt.Errorf("controller: %w", err)
	}
	return nil
}

// byID is the Session with this id, or nil.
func byID(ctx context.Context, c client.Reader, id string) (*Session, error) {
	var list SessionList
	if err := c.List(ctx, &list, client.MatchingFields{indexID: id}); err != nil {
		return nil, fmt.Errorf("sessions: %w", err)
	}
	if len(list.Items) != 1 {
		return nil, nil
	}
	return &list.Items[0], nil
}
