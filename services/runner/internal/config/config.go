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
	"k8s.io/apimachinery/pkg/util/validation"

	"github.com/rangelight/rangelight/configs"
)

const (
	mounted = "/etc/rangelight/runner.toml"
	prefix  = "RANGELIGHT_RUNNER_"
	// secrets is where secret://<name>#<key> resolves: the mounted file <secrets>/<name>/<key> (ADR 0049 §6).
	secrets = "/var/run/rangelight/secrets" //nolint:gosec // a mount path, not a credential
)

var (
	dnsName     = regexp.MustCompile(`^[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?$`)
	pinnedImage = regexp.MustCompile(`^[a-z0-9.-]+(:[0-9]+)?(/[a-z0-9._-]+)+:[A-Za-z0-9._-]+@sha256:[0-9a-f]{64}$`)
	// A NATS subject token (the same rule as rl_domain::subject::token): it can't widen or split a subject.
	subjectToken = regexp.MustCompile(`^[^.*>\s\x00-\x1f\x7f]+$`)
	secretRef    = regexp.MustCompile(`^secret://([a-z0-9]([-a-z0-9.]{0,251}[a-z0-9])?)#([-._a-zA-Z0-9]{1,253})$`)
	repoName     = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$`)
	logLevels    = map[string]slog.Level{"debug": slog.LevelDebug, "info": slog.LevelInfo, "warn": slog.LevelWarn, "error": slog.LevelError}
)

// Config is the runner's validated config.
type Config struct {
	// Namespace is the runner's own namespace, where Sessions live.
	Namespace string `toml:"namespace"`
	// Listen is the address of /auth/git and /healthz.
	Listen string `toml:"listen"`
	// MirrorURL is the git mirror's base URL, without a trailing slash; empty: no mirror is deployed
	// (lanes then fail, and a deleted Session has no mirror repo to remove).
	MirrorURL string `toml:"mirror_url"`
	// MirrorTokenFile is the runner's own projected token for the mirror, an absolute path.
	MirrorTokenFile string `toml:"mirror_token_file"`
	// DNSNamespace and DNSPodLabel (key=value) select the cluster DNS pods, the lane's only DNS server.
	DNSNamespace string `toml:"dns_namespace"`
	DNSPodLabel  string `toml:"dns_pod_label"`
	// Workers is how many Sessions reconcile at once, 1 to 64.
	Workers int `toml:"workers"`
	// AgentImage is the lane agent's image, by tag and digest.
	AgentImage string `toml:"agent_image"`
	// LaneWorkspaceSize, LaneTmpSize and LaneEphemeralStorage bound the agent pod's disk
	// (Kubernetes quantities; Quantities parses them).
	LaneWorkspaceSize    string `toml:"lane_workspace_size"`
	LaneTmpSize          string `toml:"lane_tmp_size"`
	LaneEphemeralStorage string `toml:"lane_ephemeral_storage"`
	// NATSURL is the control plane's NATS server (ADR 0021): tls://, or nats:// to a Service in
	// this cluster, since the runner's token travels to it.
	NATSURL string `toml:"nats_url"`
	// NATSTokenFile is the runner's projected token for NATS (audience rangelight-nats), an
	// absolute path, read again at every connect.
	NATSTokenFile string `toml:"nats_token_file"`
	// Org and RunnerID are the subject tokens of the runner protocol: rl.<org>.runner.<runner_id>.*.
	Org      string `toml:"org"`
	RunnerID string `toml:"runner_id"`
	// LogLevel is debug, info, warn or error.
	LogLevel string `toml:"log_level"`
	// GitHubAppID and GitHubAppPrivateKey are secret://<name>#<key> references to the GitHub App
	// that publishes lanes' work branches; both empty: publishing is off.
	GitHubAppID         string `toml:"github_app_id"`
	GitHubAppPrivateKey string `toml:"github_app_private_key"`
	// GitHubAPIURL is the GitHub API's base URL, without a trailing slash.
	GitHubAPIURL string `toml:"github_api_url"`
	// PublishImage is the git mirror's image, by tag and digest: the publish Job runs its publish.sh.
	PublishImage string `toml:"publish_image"`
	// Upstreams are the git mirror's repos' upstreams, "<name>=<https URL> ...": a session's work
	// branch is published to its repo's upstream.
	Upstreams string `toml:"upstreams"`
}

// GitHubApp is the files of the GitHub App's id and private key, and false when publishing is off.
func (c Config) GitHubApp() (string, string, bool) {
	return secretFile(c.GitHubAppID), secretFile(c.GitHubAppPrivateKey), c.GitHubAppID != ""
}

// UpstreamURLs is Upstreams by repo name. Load validated it.
func (c Config) UpstreamURLs() map[string]string {
	out := map[string]string{}
	for _, kv := range strings.Fields(c.Upstreams) {
		name, u, _ := strings.Cut(kv, "=")
		out[name] = u
	}
	return out
}

func loopback(host string) bool {
	ip := net.ParseIP(host)
	return host == "localhost" || ip != nil && ip.IsLoopback()
}

func validUpstreams(upstreams string) bool {
	for _, kv := range strings.Fields(upstreams) {
		name, raw, _ := strings.Cut(kv, "=")
		u, err := url.Parse(raw)
		if !repoName.MatchString(name) || strings.HasSuffix(name, ".git") || err != nil || u.Scheme != "https" ||
			u.Host == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" ||
			len(strings.Split(strings.Trim(u.Path, "/"), "/")) != 2 {
			return false
		}
	}
	return true
}

// secretFile is the mounted file of a secret://<name>#<key> reference, or "".
func secretFile(ref string) string {
	m := secretRef.FindStringSubmatch(ref)
	if m == nil {
		return ""
	}
	return filepath.Join(secrets, m[1], m[3])
}

// Quantities are the lane's disk sizes: workspace, tmp and ephemeral storage. Load validated them.
func (c Config) Quantities() (resource.Quantity, resource.Quantity, resource.Quantity) {
	size := func(v string) resource.Quantity {
		q, _ := resource.ParseQuantity(v) // validated by Load
		return q
	}
	return size(c.LaneWorkspaceSize), size(c.LaneTmpSize), size(c.LaneEphemeralStorage)
}

// DNSLabel is DNSPodLabel's key and value. Load validated it.
func (c Config) DNSLabel() (string, string) {
	k, v, _ := strings.Cut(c.DNSPodLabel, "=")
	return k, v
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
	c.GitHubAPIURL = strings.TrimRight(c.GitHubAPIURL, "/")
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
		"dns_namespace":          &c.DNSNamespace, "dns_pod_label": &c.DNSPodLabel,
		"nats_url": &c.NATSURL, "nats_token_file": &c.NATSTokenFile, "org": &c.Org, "runner_id": &c.RunnerID,
		"github_app_id": &c.GitHubAppID, "github_app_private_key": &c.GitHubAppPrivateKey,
		"github_api_url": &c.GitHubAPIURL, "upstreams": &c.Upstreams, "publish_image": &c.PublishImage,
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

// natsURL reports whether raw is one NATS server the runner's token may travel to: tls://, or
// plain nats:// only to a Service in this cluster by its full name (<name>.<namespace>.svc.cluster.local)
// or loopback (rl_nats::check_url's rule). Not a bare <name>.<namespace>.svc: the runner gives this URL
// to the lanes, which resolve only exact names. A remote runner needs tls://. One server: url.Parse
// takes a comma in the host, and nats.Connect would split it and send the token to each part.
func natsURL(raw string) bool {
	n, err := url.Parse(raw)
	if err != nil || strings.ContainsAny(raw, ",?#") || n.User != nil || strings.Trim(n.Path, "/") != "" {
		return false
	}
	host := n.Hostname()
	if host != "::1" && !natsHost.MatchString(host) {
		return false
	}
	if p := n.Port(); p != "" {
		if port, err := strconv.Atoi(p); err != nil || port < 1 || port > 65535 {
			return false
		}
	}
	inCluster := host == "localhost" || host == "127.0.0.1" || host == "::1" ||
		len(host) > len(".svc.cluster.local") && strings.HasSuffix(host, ".svc.cluster.local")
	return n.Scheme == "tls" || n.Scheme == "nats" && inCluster
}

// natsHost is a host name a NATS URL may name: letters, digits, dots and dashes, nothing else.
var natsHost = regexp.MustCompile(`^[-.a-zA-Z0-9]+$`)

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
	check(c.MirrorURL == "" || err == nil && (u.Scheme == "http" || u.Scheme == "https") && u.Host != "" && u.User == nil &&
		u.RawQuery == "" && u.Fragment == "", "mirror_url", "empty, or an http(s) URL without credentials, query or fragment")
	check(pinnedImage.MatchString(c.AgentImage) && !strings.Contains(c.AgentImage, ":latest@"),
		"agent_image", "an image by tag and digest, not latest")
	check(filepath.IsAbs(c.MirrorTokenFile) && filepath.Clean(c.MirrorTokenFile) == c.MirrorTokenFile, "mirror_token_file", "a clean absolute path")
	check(dnsName.MatchString(c.DNSNamespace), "dns_namespace", "a namespace name")
	k, v, found := strings.Cut(c.DNSPodLabel, "=")
	check(found && len(validation.IsQualifiedName(k)) == 0 && v != "" && len(validation.IsValidLabelValue(v)) == 0,
		"dns_pod_label", "one label as key=value, like k8s-app=kube-dns")
	check(c.Workers >= 1 && c.Workers <= 64, "workers", "1 to 64")
	sizes := map[string]string{"lane_workspace_size": c.LaneWorkspaceSize, "lane_tmp_size": c.LaneTmpSize, "lane_ephemeral_storage": c.LaneEphemeralStorage}
	for key, v := range sizes {
		q, err := resource.ParseQuantity(v)
		check(err == nil && q.Sign() > 0, key, "a positive size like 10Gi")
	}
	check(natsURL(c.NATSURL), "nats_url",
		"tls://<host>[:<port>], or nats:// to a Service in this cluster by its full name (<name>.<namespace>.svc.cluster.local: lanes can't resolve a bare .svc), without credentials")
	check(filepath.IsAbs(c.NATSTokenFile) && filepath.Clean(c.NATSTokenFile) == c.NATSTokenFile, "nats_token_file", "a clean absolute path")
	check(subjectToken.MatchString(c.Org), "org", "a subject token: no '.', '*', '>' or whitespace")
	check(subjectToken.MatchString(c.RunnerID), "runner_id", "a subject token: no '.', '*', '>' or whitespace")
	_, ok := logLevels[c.LogLevel]
	check(ok, "log_level", "debug, info, warn or error")
	c.validateGitHub(check)
	if err := errors.Join(bad...); err != nil {
		return fmt.Errorf("invalid config: %w", err)
	}
	return nil
}

func (c *Config) validateGitHub(check func(ok bool, key, want string)) {
	on := c.GitHubAppID != "" || c.GitHubAppPrivateKey != ""
	check(!on || secretFile(c.GitHubAppID) != "" && secretFile(c.GitHubAppPrivateKey) != "",
		"github_app_id and github_app_private_key", "both secret://<name>#<key>, or both empty")
	// http only to this host itself: a test's fake GitHub. The App's tokens never cross a network in clear.
	g, err := url.Parse(c.GitHubAPIURL)
	check(err == nil && (g.Scheme == "https" || g.Scheme == "http" && loopback(g.Hostname())) && g.Host != "" &&
		g.User == nil && g.RawQuery == "" && g.Fragment == "",
		"github_api_url", "an https URL (http only on loopback) without credentials, query or fragment")
	check(!on || c.Upstreams != "", "upstreams", "at least one repo when the GitHub App is set")
	check(!on || pinnedImage.MatchString(c.PublishImage), "publish_image", "the git mirror's image by tag and digest, when the GitHub App is set")
	check(validUpstreams(c.Upstreams), "upstreams", "<name>=https://<host>/<owner>/<repo>.git, separated by spaces")
}
