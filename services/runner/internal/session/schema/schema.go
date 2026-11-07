// Package schema embeds the rangelight.yaml JSON Schema the runner validates profiles against.
//
// An embed directive can't reach outside the module, so this directory holds a byte-for-byte copy of
// api/jsonschema/rangelight.v1alpha1.schema.json. Never edit it here: change api/ and copy it
// over; `scripts/check.sh spec` fails when the two differ.
package schema

import _ "embed"

// ProfileV1alpha1 is the rangelight.yaml v1alpha1 JSON Schema (ADR 0041: the runner refuses an
// invalid profile with INVALID_PROFILE).
//
//go:embed rangelight.v1alpha1.schema.json
var ProfileV1alpha1 []byte
