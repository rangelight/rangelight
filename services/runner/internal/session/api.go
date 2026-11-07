// Package session runs Sessions (ADR 0012, 0015, 0016, 0018, 0046): for each one a locked-down lane
// namespace with default-deny egress, the member's Claude home, the ticket and the profile read from
// the base branch, and a hardened agent pod. It also answers the git mirror's /auth/git (GitAuth).
package session

import (
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/runtime"
	"k8s.io/apimachinery/pkg/runtime/schema"
)

// GroupVersion is the Session API's group and version (ADR 0051: the group is rangelight.dev).
// The schema the API server enforces is the chart's crds/session.yaml.
var GroupVersion = schema.GroupVersion{Group: "rangelight.dev", Version: "v1alpha1"}

// AddToScheme registers Session and SessionList in a scheme.
func AddToScheme(s *runtime.Scheme) error {
	s.AddKnownTypes(GroupVersion, &Session{}, &SessionList{})
	metav1.AddToGroupVersion(s, GroupVersion)
	return nil
}

// Session is one ticket's lane: the runner's local desired state for it (ADR 0012, ADR 0018).
//
// +kubebuilder:object:generate=true
// +kubebuilder:object:root=true
type Session struct {
	metav1.TypeMeta   `json:",inline"`
	metav1.ObjectMeta `json:"metadata,omitempty"`

	Spec   Spec   `json:"spec"`
	Status Status `json:"status,omitempty"`
}

// SessionList is a list of Sessions.
//
// +kubebuilder:object:generate=true
// +kubebuilder:object:root=true
type SessionList struct { //nolint:revive // Kubernetes names a kind's list <Kind>List
	metav1.TypeMeta `json:",inline"`
	metav1.ListMeta `json:"metadata,omitempty"`

	Items []Session `json:"items"`
}

// Spec is what the control plane decided for a session (runner.v1alpha1.SessionDesired).
//
// +kubebuilder:object:generate=true
type Spec struct {
	// ID is the session's lowercase UUIDv7.
	ID string `json:"id"`
	// Ticket is mounted in the lane as /etc/rangelight/ticket.json.
	Ticket Ticket `json:"ticket"`
	// Repo is the git mirror's name for the repo, served at /<repo>.git.
	Repo string `json:"repo"`
	// BaseRef is the approved base branch the profile is read from (ADR 0041).
	BaseRef string `json:"baseRef"`
	// Member is whose Claude home the lane mounts (ADR 0046 amendment).
	Member string `json:"member"`
}

// Ticket is the ticket a lane works on, as services/agent reads it.
//
// +kubebuilder:object:generate=true
type Ticket struct {
	Key         string   `json:"key"`
	Title       string   `json:"title"`
	Description string   `json:"description"`
	Criteria    []string `json:"criteria"`
}

// Status is what the runner observed.
//
// +kubebuilder:object:generate=true
type Status struct {
	Phase Phase `json:"phase,omitempty"`
	// Message says why the session failed.
	Message string `json:"message,omitempty"`
	// ProfileCommit is the base-branch commit the profile was read from.
	ProfileCommit string `json:"profileCommit,omitempty"`
}

// Phase is where a session's pod is: Pending, then Starting once its namespace exists, then Hot
// once the agent pod is Ready. Failed is final.
type Phase string

// The phases.
const (
	PhasePending  Phase = "Pending"
	PhaseStarting Phase = "Starting"
	PhaseHot      Phase = "Hot"
	PhaseFailed   Phase = "Failed"
)
