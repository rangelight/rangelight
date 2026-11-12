// The runner: the only Kubernetes-aware part of Rangelight (ADR 0018). It takes Sessions from the
// control plane over NATS, runs them, reports their phase back, and answers the git mirror's /auth/git.
package main

import (
	"fmt"
	"log/slog"
	"os"
	"time"

	"github.com/go-logr/logr"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/manager"

	"github.com/rangelight/rangelight/services/runner/internal/config"
	"github.com/rangelight/rangelight/services/runner/internal/github"
	"github.com/rangelight/rangelight/services/runner/internal/session"
)

func main() {
	if err := run(); err != nil {
		slog.Error("runner stopped", "error", err.Error())
		os.Exit(1)
	}
}

func run() error {
	slog.SetDefault(slog.New(slog.NewJSONHandler(os.Stdout, nil)))
	cfg, err := config.Load()
	if err != nil {
		return fmt.Errorf("config: %w", err)
	}
	log := slog.New(slog.NewJSONHandler(os.Stdout, &slog.HandlerOptions{Level: cfg.Level()}))
	slog.SetDefault(log)
	ctrl.SetLogger(logr.FromSlogHandler(log.Handler()))
	rest, err := ctrl.GetConfig()
	if err != nil {
		return fmt.Errorf("kubernetes: %w", err)
	}
	mgr, err := session.NewManager(rest, cfg.Namespace)
	if err != nil {
		return fmt.Errorf("session: %w", err)
	}
	report, err := session.AddProtocol(mgr, cfg)
	if err != nil {
		return fmt.Errorf("session: %w", err)
	}
	workspace, tmp, ephemeral := cfg.Quantities()
	dnsKey, dnsValue := cfg.DNSLabel()
	var app *github.App // nil: publishing is off
	if idFile, keyFile, on := cfg.GitHubApp(); on {
		app = &github.App{API: cfg.GitHubAPIURL, IDFile: idFile, KeyFile: keyFile}
	}
	r := &session.Reconciler{Client: mgr.GetClient(), Reader: mgr.GetAPIReader(), Report: report, Options: session.Options{
		AgentImage: cfg.AgentImage, MirrorURL: cfg.MirrorURL, MirrorTokenFile: cfg.MirrorTokenFile,
		Workspace: workspace, Tmp: tmp, EphemeralStorage: ephemeral, Workers: cfg.Workers,
		DNS:    session.DNS{Namespace: cfg.DNSNamespace, LabelKey: dnsKey, LabelValue: dnsValue},
		GitHub: app, Upstreams: cfg.UpstreamURLs(), PublishImage: cfg.PublishImage,
	}}
	if err := r.SetupWithManager(mgr); err != nil {
		return fmt.Errorf("session: %w", err)
	}
	srv := session.NewServer(cfg.Listen, &session.GitAuth{Client: mgr.GetClient(), Cache: mgr.GetCache(), Reader: mgr.GetAPIReader(), Namespace: cfg.Namespace})
	drain := 10 * time.Second
	if err := mgr.Add(&manager.Server{Name: "http", Server: srv, ShutdownTimeout: &drain}); err != nil {
		return fmt.Errorf("http: %w", err)
	}
	slog.Info("runner starting", "namespace", cfg.Namespace, "listen", cfg.Listen, "runner_id", cfg.RunnerID, "publishing", app != nil)
	if err := mgr.Start(ctrl.SetupSignalHandler()); err != nil {
		return fmt.Errorf("manager: %w", err)
	}
	return nil
}
