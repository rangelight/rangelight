package session

import (
	"errors"
	"strings"
	"testing"
	"unicode/utf8"

	corev1 "k8s.io/api/core/v1"
)

// The agent container decides the phase; the pod's Ready condition alone never makes it Hot.
func TestPodPhase(t *testing.T) {
	readyPod := []corev1.PodCondition{{Type: corev1.PodReady, Status: corev1.ConditionTrue}}
	agent := func(c corev1.ContainerStatus) []corev1.ContainerStatus {
		c.Name = agentName
		return []corev1.ContainerStatus{c}
	}
	tests := []struct {
		name       string
		conditions []corev1.PodCondition
		containers []corev1.ContainerStatus
		want       Phase
		fails      bool
		message    string
	}{
		{"no status yet", nil, nil, PhaseStarting, false, ""},
		{"pod Ready, no container status", readyPod, nil, PhaseStarting, false, ""},
		{"container Ready", readyPod, agent(corev1.ContainerStatus{Ready: true, State: corev1.ContainerState{Running: &corev1.ContainerStateRunning{}}}), PhaseHot, false, ""},
		{"container running, not Ready", readyPod, agent(corev1.ContainerStatus{State: corev1.ContainerState{Running: &corev1.ContainerStateRunning{}}}), PhaseStarting, false, ""},
		{"creating", nil, agent(corev1.ContainerStatus{State: corev1.ContainerState{Waiting: &corev1.ContainerStateWaiting{Reason: "ContainerCreating"}}}), PhaseStarting, false, ""},
		{"an image that won't pull", nil, agent(corev1.ContainerStatus{State: corev1.ContainerState{Waiting: &corev1.ContainerStateWaiting{Reason: "ImagePullBackOff", Message: "Back-off pulling image"}}}), PhaseStarting, false, "agent container waiting: ImagePullBackOff: Back-off pulling image"},
		{"a config error", nil, agent(corev1.ContainerStatus{State: corev1.ContainerState{Waiting: &corev1.ContainerStateWaiting{Reason: "CreateContainerConfigError", Message: "configmap not found"}}}), PhaseStarting, false, "agent container waiting: CreateContainerConfigError: configmap not found"},
		{"exited 0", nil, agent(corev1.ContainerStatus{State: corev1.ContainerState{Terminated: &corev1.ContainerStateTerminated{Reason: "Completed"}}}), PhaseStarting, false, "agent container exited: Completed (exit code 0)"},
		{"one OOM restart", nil, agent(corev1.ContainerStatus{RestartCount: 1, State: corev1.ContainerState{Waiting: &corev1.ContainerStateWaiting{Reason: "CrashLoopBackOff"}},
			LastTerminationState: corev1.ContainerState{Terminated: &corev1.ContainerStateTerminated{Reason: "OOMKilled", ExitCode: 137}}}), PhaseStarting, false, "agent container restarting (1): OOMKilled (exit code 137)"},
		{"3 crash-loop restarts", nil, agent(corev1.ContainerStatus{RestartCount: 3, State: corev1.ContainerState{Waiting: &corev1.ContainerStateWaiting{Reason: "CrashLoopBackOff"}},
			LastTerminationState: corev1.ContainerState{Terminated: &corev1.ContainerStateTerminated{Reason: "Error", ExitCode: 1}}}), "", true, ""},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, message, err := podPhase(&corev1.Pod{Status: corev1.PodStatus{Conditions: tt.conditions, ContainerStatuses: tt.containers}})
			var f *failedError
			if got != tt.want || message != tt.message || errors.As(err, &f) != tt.fails {
				t.Errorf("got %q %q, %v; want %q %q, failed %v", got, message, err, tt.want, tt.message, tt.fails)
			}
		})
	}
}

// Review: a message is cut by characters, never inside one, to the CRD's maxLength.
func TestOneLineCutsByCharacters(t *testing.T) {
	got := oneLine(strings.Repeat("a", messageMax-1) + "éé\n")
	if !utf8.ValidString(got) || utf8.RuneCountInString(got) != messageMax || !strings.HasSuffix(got, "é") {
		t.Errorf("cut to %d characters, valid UTF-8 %v, ends %q", utf8.RuneCountInString(got), utf8.ValidString(got), got[len(got)-4:])
	}
}
