package schema

import (
	"encoding/json"
	"testing"
)

func TestProfileV1alpha1IsTheV1alpha1Schema(t *testing.T) {
	var doc struct {
		ID     string `json:"$id"`
		Schema string `json:"$schema"`
	}
	if err := json.Unmarshal(ProfileV1alpha1, &doc); err != nil {
		t.Fatalf("embedded schema is not JSON: %v", err)
	}
	tests := []struct{ name, got, want string }{
		{"$id", doc.ID, "https://rangelight.dev/schemas/rangelight.v1alpha1.schema.json"},
		{"$schema", doc.Schema, "https://json-schema.org/draft/2020-12/schema"},
	}
	for _, tt := range tests {
		if tt.got != tt.want {
			t.Errorf("%s = %q, want %q", tt.name, tt.got, tt.want)
		}
	}
}
