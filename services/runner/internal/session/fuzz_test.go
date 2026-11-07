package session

import (
	"net/http/httptest"
	"strings"
	"testing"
)

// The Authorization header is network input (the mirror forwards the client's).
func FuzzBearer(f *testing.F) {
	for _, seed := range []string{"", "Bearer abc", "Basic eDphYmM=", "Basic !!", "Bearer ", "bearer x"} {
		f.Add(seed)
	}
	f.Fuzz(func(t *testing.T, header string) {
		req := httptest.NewRequest("GET", "/auth/git", nil)
		req.Header.Set("Authorization", header)
		token := bearer(req)
		if token != "" && !strings.Contains(header, " ") {
			t.Errorf("a token from a header without a scheme: %q", header)
		}
	})
}

// The TokenReview's username decides the namespace.
func FuzzServiceAccount(f *testing.F) {
	for _, seed := range []string{"system:serviceaccount:rl-a:default", "system:serviceaccount::x", "admin", "system:serviceaccount:a"} {
		f.Add(seed)
	}
	f.Fuzz(func(t *testing.T, user string) {
		ns, name, ok := serviceAccount(user)
		if ok && (ns == "" || name == "" || strings.Contains(ns, ":") || "system:serviceaccount:"+ns+":"+name != user) {
			t.Errorf("%q split into %q, %q", user, ns, name)
		}
	})
}

// rangelight.yaml comes from the repo, which the agent and the repo's authors control.
func FuzzProfile(f *testing.F) {
	p, err := newProfiles("http://mirror", "/nonexistent")
	if err != nil {
		f.Fatal(err)
	}
	for _, seed := range []string{"", "a: [", "apiVersion: \"rangelight.dev/v1alpha1\"\n", "&a [*a, *a]", "{\"spec\": 1}"} {
		f.Add([]byte(seed))
	}
	f.Fuzz(func(t *testing.T, raw []byte) {
		if js, err := p.valid(raw); err == nil && !strings.HasPrefix(string(js), "{") {
			t.Errorf("a valid profile that isn't a JSON object: %s", js)
		}
	})
}
