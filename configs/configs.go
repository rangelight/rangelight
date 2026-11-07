// Package configs holds each service's default config file (ADR 0049 §6). Go services embed theirs
// from here, since go:embed can't reach a parent directory.
package configs

import _ "embed"

// Runner is configs/runner.default.toml.
//
//go:embed runner.default.toml
var Runner []byte
