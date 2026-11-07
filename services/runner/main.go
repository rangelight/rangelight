// The runner: the only Kubernetes-aware part of Rangelight (ADR 0018). It runs Sessions and answers
// the git mirror's /auth/git.
package main

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"time"

	"github.com/go-logr/logr"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/manager"

	"github.com/rangelight/rangelight/services/runner/internal/config"
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
	workspace, tmp, ephemeral := cfg.Quantities()
	r := &session.Reconciler{Client: mgr.GetClient(), Reader: mgr.GetAPIReader(), Options: session.Options{
		AgentImage: cfg.AgentImage, MirrorURL: cfg.MirrorURL, MirrorTokenFile: cfg.MirrorTokenFile,
		Workspace: workspace, Tmp: tmp, EphemeralStorage: ephemeral, Workers: cfg.Workers,
	}}
	if err := r.SetupWithManager(mgr); err != nil {
		return fmt.Errorf("session: %w", err)
	}
	srv := session.NewServer(cfg.Listen, &session.GitAuth{Client: mgr.GetClient(), Cache: mgr.GetCache(), Namespace: cfg.Namespace})
	if err := mgr.Add(manager.RunnableFunc(func(ctx context.Context) error { return serve(ctx, srv) })); err != nil {
		return fmt.Errorf("http: %w", err)
	}
	slog.Info("runner starting", "namespace", cfg.Namespace, "listen", cfg.Listen)
	if err := mgr.Start(ctrl.SetupSignalHandler()); err != nil {
		return fmt.Errorf("manager: %w", err)
	}
	return nil
}

// serve runs srv until ctx ends, then drains it for up to 10 seconds.
func serve(ctx context.Context, srv *http.Server) error {
	go func() {
		<-ctx.Done()
		stop, cancel := context.WithTimeout(context.WithoutCancel(ctx), 10*time.Second)
		defer cancel()
		_ = srv.Shutdown(stop)
	}()
	if err := srv.ListenAndServe(); !errors.Is(err, http.ErrServerClosed) {
		return fmt.Errorf("http: %w", err)
	}
	return nil
}
