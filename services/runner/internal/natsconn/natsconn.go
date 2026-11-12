// Package natsconn signs the runner in to NATS as itself (ADR 0021 amendment). It presents its
// projected ServiceAccount token (audience rangelight-nats), read again at every connect and
// reconnect, so the kubelet's rotation and the one-hour user JWT the api's auth callout issues
// never end the connection, and a refused sign-in never stops its reconnecting. Its replies arrive under its own inbox prefix, the only one its
// permissions let it subscribe to (rl_domain::subject::runner, test/nats-permissions.json).
package natsconn

import (
	"log/slog"
	"os"
	"strings"

	"github.com/nats-io/nats.go"
)

// Options sign the runner runner in with the token in tokenFile and take its replies under
// Inbox(runner).
func Options(tokenFile, runner string) []nats.Option {
	return []nats.Option{
		nats.TokenHandler(func() string {
			token, err := os.ReadFile(tokenFile) //nolint:gosec // the configured token file
			if err != nil {
				slog.Warn("nats: can't read the token", "file", tokenFile, "error", err.Error())
				return ""
			}
			return strings.TrimSpace(string(token))
		}),
		nats.CustomInboxPrefix(Inbox(runner)),
		// A refused sign-in (the api's callout restarting, a token not yet rotated) is retried like any
		// other failure: by default nats.go stops reconnecting for good after two in a row.
		nats.IgnoreAuthErrorAbort(),
	}
}

// Inbox is the runner's inbox prefix: rl_domain::subject::inbox of its consumer name.
func Inbox(runner string) string { return "_INBOX_runner-" + runner }
