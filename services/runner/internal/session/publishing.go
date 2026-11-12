package session

import (
	"sync"
	"time"
)

// publishing is what the runner keeps in memory: the installation token of each Job it released
// (to open the pull request, then revoke it), and the backoff of each session whose publish fails.
//
// After a restart a Job's token is read back from its Secret (heldToken): none is lost or minted twice.
//
// ponytail: revoked and handled grow by one Job name per publish until the runner restarts.
// Upgrade: drop a name once its Job's TTL has removed it.
type publishing struct {
	mu      sync.Mutex
	tokens  map[string]string  // by Job name
	revoked map[string]bool    // Jobs whose token is revoked: its Secret holds a dead one
	handled map[string]bool    // ended Jobs already acted on, so a stale read doesn't act twice
	fails   map[string]failure // by session id
}

type failure struct {
	n    int
	next time.Time
}

func newPublishing() *publishing {
	return &publishing{tokens: map[string]string{}, revoked: map[string]bool{}, handled: map[string]bool{}, fails: map[string]failure{}}
}

// wait is how long a session's failing publish waits before its next try.
func (p *publishing) wait(id string) time.Duration {
	p.mu.Lock()
	defer p.mu.Unlock()
	return time.Until(p.fails[id].next)
}

// fail counts one more failed try and returns the wait before the next.
func (p *publishing) fail(id string) time.Duration {
	p.mu.Lock()
	defer p.mu.Unlock()
	f := p.fails[id]
	wait := backoff(f.n)
	p.fails[id] = failure{n: f.n + 1, next: time.Now().Add(wait)}
	return wait
}

func (p *publishing) reset(id string) {
	p.mu.Lock()
	defer p.mu.Unlock()
	delete(p.fails, id)
}

func (p *publishing) mark(set map[string]bool, job string) {
	p.mu.Lock()
	defer p.mu.Unlock()
	set[job] = true
}

func (p *publishing) is(set map[string]bool, job string) bool {
	p.mu.Lock()
	defer p.mu.Unlock()
	return set[job]
}

func (p *publishing) token(job string) string {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.tokens[job]
}

func (p *publishing) remember(job, token string) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.tokens[job] = token
}

// forget drops a Job's token and returns it ("" if it was never held).
func (p *publishing) forget(job string) string {
	p.mu.Lock()
	defer p.mu.Unlock()
	t := p.tokens[job]
	delete(p.tokens, job)
	return t
}
