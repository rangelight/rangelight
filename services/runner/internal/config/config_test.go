package config

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const image = "ghcr.io/rangelight/agent:1@sha256:1111111111111111111111111111111111111111111111111111111111111111"

func TestLoadLayers(t *testing.T) {
	file := filepath.Join(t.TempDir(), "runner.toml")
	if err := os.WriteFile(file, []byte("agent_image = \""+image+"\"\nlog_level = \"debug\"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	c, err := load(file, []string{"RANGELIGHT_RUNNER_LOG_LEVEL=warn", "RANGELIGHT_RUNNER_MIRROR_URL=http://mirror:8080/", "RANGELIGHT_RUNNER_WORKERS=8", "HOME=/x"})
	if err != nil {
		t.Fatal(err)
	}
	want := Config{ //nolint:gosec // a token file path, not a token
		Namespace: "rangelight-runner", Listen: ":8080", MirrorURL: "http://mirror:8080",
		MirrorTokenFile: "/var/run/secrets/rangelight/git-mirror/token", Workers: 8, AgentImage: image,
		LaneWorkspaceSize: "10Gi", LaneTmpSize: "1Gi", LaneEphemeralStorage: "12Gi", LogLevel: "warn",
	}
	if c != want {
		t.Errorf("got %+v, want %+v", c, want)
	}
}

func TestLoadWithoutMountedFile(t *testing.T) {
	c, err := load(filepath.Join(t.TempDir(), "absent.toml"), []string{"RANGELIGHT_RUNNER_AGENT_IMAGE=" + image})
	if err != nil || c.AgentImage != image || c.MirrorURL != "http://git-mirror.rangelight-system.svc.cluster.local:8080" || c.Workers != 4 {
		t.Fatalf("%+v, %v", c, err)
	}
	workspace, tmp, ephemeral := c.Quantities()
	if workspace.String() != "10Gi" || tmp.String() != "1Gi" || ephemeral.String() != "12Gi" {
		t.Errorf("quantities %s %s %s", &workspace, &tmp, &ephemeral)
	}
}

func TestLoadRefuses(t *testing.T) {
	tests := []struct{ name, file, env, want string }{
		{"no agent image", "", "RANGELIGHT_RUNNER_AGENT_IMAGE=", "agent_image"},
		{"an image without a digest", "", "RANGELIGHT_RUNNER_AGENT_IMAGE=ghcr.io/rangelight/agent:1", "agent_image"},
		{"a latest image", "", "RANGELIGHT_RUNNER_AGENT_IMAGE=" + strings.Replace(image, ":1@", ":latest@", 1), "agent_image"},
		{"an unknown variable", "", "RANGELIGHT_RUNNER_TOKEN=x", "RANGELIGHT_RUNNER_TOKEN: unknown setting"},
		{"an unknown key in the file", "password = \"x\"\n", "HOME=/x", "password"},
		{"a file that isn't TOML", "{", "HOME=/x", "runner.toml"},
		{"a bad namespace", "", "RANGELIGHT_RUNNER_NAMESPACE=Runner", "namespace"},
		{"a bad listen address", "", "RANGELIGHT_RUNNER_LISTEN=8080", "listen"},
		{"a mirror URL with credentials", "", "RANGELIGHT_RUNNER_MIRROR_URL=http://u:p@mirror", "mirror_url"},
		{"a mirror URL that isn't http", "", "RANGELIGHT_RUNNER_MIRROR_URL=file:///srv/git", "mirror_url"},
		{"a bad log level", "", "RANGELIGHT_RUNNER_LOG_LEVEL=loud", "log_level"},
		{"a relative token file", "", "RANGELIGHT_RUNNER_MIRROR_TOKEN_FILE=token", "mirror_token_file"},
		{"a token file with ..", "", "RANGELIGHT_RUNNER_MIRROR_TOKEN_FILE=/var/../token", "mirror_token_file"},
		{"no workers", "", "RANGELIGHT_RUNNER_WORKERS=0", "workers"},
		{"workers not a number", "", "RANGELIGHT_RUNNER_WORKERS=many", "RANGELIGHT_RUNNER_WORKERS: want a number"},
		{"a workspace that isn't a size", "", "RANGELIGHT_RUNNER_LANE_WORKSPACE_SIZE=big", "lane_workspace_size"},
		{"no tmp", "", "RANGELIGHT_RUNNER_LANE_TMP_SIZE=0", "lane_tmp_size"},
		{"a negative ephemeral storage", "", "RANGELIGHT_RUNNER_LANE_EPHEMERAL_STORAGE=-1Gi", "lane_ephemeral_storage"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			file := filepath.Join(t.TempDir(), "runner.toml")
			if err := os.WriteFile(file, []byte(tt.file), 0o600); err != nil {
				t.Fatal(err)
			}
			env := []string{"RANGELIGHT_RUNNER_AGENT_IMAGE=" + image, tt.env}
			if _, err := load(file, env); err == nil || !strings.Contains(err.Error(), tt.want) {
				t.Errorf("error %v, want one naming %q", err, tt.want)
			}
		})
	}
}
