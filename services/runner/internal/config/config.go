// Package config is the runner's typed config, and the only package that reads the environment
// (ADR 0049 §6): configs/runner.default.toml, then the mounted /etc/rangelight/runner.toml if
// present, then RANGELIGHT_RUNNER_<KEY> variables. Every value is validated at load.
package config

import (
	"bytes"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"

	"github.com/pelletier/go-toml/v2"
	"k8s.io/apimachinery/pkg/api/resource"

	"github.com/rangelight/rangelight/configs"
)

const (
	mounted = "/etc/rangelight/runner.toml"
	prefix  = "RANGELIGHT_RUNNER_"
)

var (
	dnsName     = regexp.MustCompile(`^[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?$`)
	pinnedImage = regexp.MustCompile(`^[a-z0-9.-]+(:[0-9]+)?(/[a-z0-9._-]+)+:[A-Za-z0-9._-]+@sha256:[0-9a-f]{64}$`)
	logLevels   = map[string]slog.Level{"debug": slog.LevelDebug, "info": slog.LevelInfo, "warn": slog.LevelWarn, "error": slog.LevelError}
)

// Config is the runner's validated config.
type Config struct {
	// Namespace is the runner's own namespace, where Sessions live.
	Namespace string `toml:"namespace"`
	// Listen is the address of /auth/git and /healthz.
	Listen string `toml:"listen"`
	// MirrorURL is the git mirror's base URL, without a trailing slash.
	MirrorURL string `toml:"mirror_url"`
	// MirrorTokenFile is the runner's own projected token for the mirror, an absolute path.
	MirrorTokenFile string `toml:"mirror_token_file"`
	// Workers is how many Sessions reconcile at once, 1 to 64.
	Workers int `toml:"workers"`
	// AgentImage is the lane agent's image, by tag and digest.
	AgentImage string `toml:"agent_image"`
	// LaneWorkspaceSize, LaneTmpSize and LaneEphemeralStorage bound the agent pod's disk
	// (Kubernetes quantities; Quantities parses them).
	LaneWorkspaceSize    string `toml:"lane_workspace_size"`
	LaneTmpSize          string `toml:"lane_tmp_size"`
	LaneEphemeralStorage string `toml:"lane_ephemeral_storage"`
	// LogLevel is debug, info, warn or error.
	LogLevel string `toml:"log_level"`
}

// Quantities are the lane's disk sizes: workspace, tmp and ephemeral storage. Load validated them.
func (c Config) Quantities() (resource.Quantity, resource.Quantity, resource.Quantity) {
	size := func(v string) resource.Quantity {
		q, _ := resource.ParseQuantity(v) // validated by Load
		return q
	}
	return size(c.LaneWorkspaceSize), size(c.LaneTmpSize), size(c.LaneEphemeralStorage)
}

// Level is the slog level of LogLevel.
func (c Config) Level() slog.Level { return logLevels[c.LogLevel] }

// Load reads the config from its three layers. The error names every bad key.
func Load() (Config, error) { return load(mounted, os.Environ()) }

func load(file string, environ []string) (Config, error) {
	var c Config
	if err := decode(configs.Runner, &c); err != nil {
		return c, fmt.Errorf("configs/runner.default.toml: %w", err)
	}
	raw, err := os.ReadFile(file) //nolint:gosec // a fixed path, or a test's
	switch {
	case err == nil:
		if err := decode(raw, &c); err != nil {
			return c, fmt.Errorf("%s: %w", file, err)
		}
	case !errors.Is(err, os.ErrNotExist):
		return c, fmt.Errorf("config: %w", err)
	}
	if err := c.fromEnv(environ); err != nil {
		return c, err
	}
	c.MirrorURL = strings.TrimRight(c.MirrorURL, "/")
	return c, c.validate()
}

func decode(raw []byte, c *Config) error {
	d := toml.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	err := d.Decode(c)
	var unknown *toml.StrictMissingError
	if errors.As(err, &unknown) {
		return fmt.Errorf("unknown key: %s", unknown.String())
	}
	if err != nil {
		return fmt.Errorf("invalid config: %w", err)
	}
	return nil
}

func (c *Config) fields() map[string]*string {
	return map[string]*string{
		"namespace": &c.Namespace, "listen": &c.Listen, "mirror_url": &c.MirrorURL,
		"mirror_token_file": &c.MirrorTokenFile, "agent_image": &c.AgentImage, "log_level": &c.LogLevel,
		"lane_workspace_size": &c.LaneWorkspaceSize, "lane_tmp_size": &c.LaneTmpSize,
		"lane_ephemeral_storage": &c.LaneEphemeralStorage,
	}
}

func (c *Config) fromEnv(environ []string) error {
	fields := c.fields()
	for _, kv := range environ {
		name, value, _ := strings.Cut(kv, "=")
		key, ok := strings.CutPrefix(name, prefix)
		if !ok {
			continue
		}
		key = strings.ToLower(key)
		if key == "workers" {
			n, err := strconv.Atoi(value)
			if err != nil {
				return fmt.Errorf("%s: want a number", name)
			}
			c.Workers = n
			continue
		}
		field, known := fields[key]
		if !known {
			return fmt.Errorf("%s: unknown setting", name)
		}
		*field = value
	}
	return nil
}

func (c *Config) validate() error {
	var bad []error
	check := func(ok bool, key, want string) {
		if !ok {
			bad = append(bad, fmt.Errorf("%s: want %s", key, want))
		}
	}
	check(dnsName.MatchString(c.Namespace), "namespace", "a namespace name")
	_, _, err := net.SplitHostPort(c.Listen)
	check(err == nil, "listen", "host:port")
	u, err := url.Parse(c.MirrorURL)
	check(err == nil && (u.Scheme == "http" || u.Scheme == "https") && u.Host != "" && u.User == nil &&
		u.RawQuery == "" && u.Fragment == "", "mirror_url", "an http(s) URL without credentials, query or fragment")
	check(pinnedImage.MatchString(c.AgentImage) && !strings.Contains(c.AgentImage, ":latest@"),
		"agent_image", "an image by tag and digest, not latest")
	check(filepath.IsAbs(c.MirrorTokenFile) && filepath.Clean(c.MirrorTokenFile) == c.MirrorTokenFile, "mirror_token_file", "a clean absolute path")
	check(c.Workers >= 1 && c.Workers <= 64, "workers", "1 to 64")
	sizes := map[string]string{"lane_workspace_size": c.LaneWorkspaceSize, "lane_tmp_size": c.LaneTmpSize, "lane_ephemeral_storage": c.LaneEphemeralStorage}
	for key, v := range sizes {
		q, err := resource.ParseQuantity(v)
		check(err == nil && q.Sign() > 0, key, "a positive size like 10Gi")
	}
	_, ok := logLevels[c.LogLevel]
	check(ok, "log_level", "debug, info, warn or error")
	if err := errors.Join(bad...); err != nil {
		return fmt.Errorf("invalid config: %w", err)
	}
	return nil
}
