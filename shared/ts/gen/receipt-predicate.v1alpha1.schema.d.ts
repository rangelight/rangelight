// Generated from api/jsonschema/ by scripts/check.sh gen. Do not edit.

/**
 * The predicate of an in-toto Statement whose predicateType is https://rangelight.dev/receipt/v1alpha1. The Statement is wrapped in a DSSE envelope and signed with the org key.
 */
export type RangelightReceiptPredicateV1Alpha1 = {
  session: {
    /**
     * UUIDv7 session id.
     */
    id: string;
    org: string;
    team: string;
    /**
     * OIDC subject of the human who owns the session.
     */
    owner: string;
    /**
     * build: a Rangelight lane. verify: verification of an external branch (ADR 0048).
     */
    kind: "build" | "verify";
    ticket?: {
      tracker: "linear" | "github-issues";
      id: string;
      url: string;
    };
    /**
     * Optional id of the plan session the ticket came from (ADR 0007).
     */
    planSession?: string;
    pullRequest?: string;
    startedAt: string;
    finishedAt: string;
    /**
     * rangelight: a Rangelight lane produced the work. external: Rangelight only verified a branch it didn't produce (ADR 0048); agent-side fields are absent and shown as 'not recorded'.
     */
    producer?: "rangelight" | "external";
    /**
     * What started the session or verification (ADR 0048: lanes started over MCP are tagged mcp).
     */
    source?: "label" | "workbench" | "dispatcher" | "mcp" | "pull-request";
  };
  contract?: {
    version: number;
    digest: Digest;
  };
  /**
   * Items: A criterion is proven only by observed evidence from a gate Job (ADR 0041). Claimed evidence may be listed but never proves.
   */
  criteria: {
    id: string;
    text: string;
    status: "proven" | "failed" | "claimed" | "waived";
    /**
     * Decision id; required when status is waived.
     */
    waivedBy?: string;
    proofs: Evidence[];
    /**
     * Name of the test that proves it: contains AC<n>, was added by this branch (absent on main), and ran in Rangelight's own run (ADR 0045 §3).
     */
    test?: string;
  }[];
  /**
   * One entry per diff hunk in the PR.
   */
  provenance?: {
    repo: string;
    path: string;
    /**
     * Unified-diff hunk header, e.g. @@ -10,6 +10,9 @@.
     */
    hunk: string;
    author: "agent" | "human" | "mixed";
    /**
     * agent:<session>/<lane>.
     */
    agent?: string;
    model?: string;
    promptDigest?: Digest;
    skillDigests?: Digest[];
    /**
     * Criterion id this hunk serves.
     */
    criterion?: string;
    humanTouchedBy?: string;
  }[];
  capabilities?: {
    kind: "llm" | "mcp-tool" | "egress-host" | "git" | "package-registry";
    name: string;
    granted: boolean;
    /**
     * Policy id or decision id that granted it.
     */
    grantSource?: string;
    /**
     * Number of calls.
     */
    used: number;
    /**
     * Number of denied attempts.
     */
    blocked: number;
  }[];
  /**
   * Each decision is a control-plane event naming the authenticated user. WebAuthn signatures are deferred (ADR 0022 amendment).
   */
  decisions?: {
    id: string;
    /**
     * UUIDv7 of the event in the session chain.
     */
    eventId: string;
    kind: "escalation" | "budget" | "blocked-tool" | "gate-skip" | "config-adopt" | "preemption" | "other";
    decidedBy: string;
    decidedAt: string;
    choice: string;
    reason?: string;
  }[];
  config: {
    /**
     * rangelight-config commit the contract was frozen against (ADR 0005).
     */
    snapshotSha: string;
    profileDigest: Digest1;
  };
  runtime: {
    /**
     * runc, gvisor, kata, ... (ADR 0015).
     */
    runtimeClass: string;
    podSecurity: "restricted";
    images: Artifact[];
    rangelightVersion?: string;
    /**
     * AgentRuntime implementation (ADR 0046). Absent for external receipts.
     */
    agentRuntime?: "sdk" | "cli";
    /**
     * How the lane paid for Claude (ADR 0046). Never a token or key value.
     */
    auth?: "team-key" | "member-key" | "claude-plan";
  };
  data?: {
    store: string;
    mode: "fixtures" | "masked-snapshot";
    goldenCopyDigest?: Digest;
    maskingRulesSha?: string;
    externalDeps?: {
      name: string;
      mode: "mock" | "sandbox" | "block";
    }[];
  }[];
  cost?: {
    currency: string;
    /**
     * Which credential paid, by reference only.
     */
    credentialRef?: string;
    models: {
      model: string;
      inputTokens: number;
      outputTokens: number;
      cacheReadTokens: number;
      cacheWriteTokens?: number;
      amount: string;
    }[];
    envHours: number;
    envAmount?: string;
    total: string;
  };
  /**
   * Milliseconds from session start to each readiness layer (ADR 0024).
   */
  readiness?: {
    layer: "code" | "deps" | "neighbours" | "schema" | "service" | "preview";
    ms: number;
  }[];
  chain: {
    algorithm: "sha256";
    /**
     * Hash of the last event in the session chain.
     */
    head: string;
    length: number;
    /**
     * Optional. Where an authorised verifier can fetch the event export online; the eventsBundle is the offline path.
     */
    eventsUri?: string;
    /**
     * Optional org Merkle checkpoint inclusion (ADR 0029).
     */
    checkpoint?: {
      checkpointId: string;
      root: string;
      leafIndex: number;
      proof: string[];
    };
    eventsBundle: Artifact1;
  };
  /**
   * Rangelight's own checks on the pushed head, run in the observed gate Job (ADR 0045 §3).
   *
   * @minItems 4
   */
  checks: [
    {
      id: "tests-pass" | "main-tests-still-pass" | "committed-and-pushed" | "merges-cleanly";
      ok: boolean;
      note: string;
      gateJob?: string;
      commit?: string;
      /**
       * The main SHA the merge-based checks ran against.
       */
      mainCommit?: string;
      /**
       * Who ran the check: Rangelight's own gate Job, or the rangelight/verify GitHub Action in the repo's CI (ADR 0056 §1).
       */
      observer?: "rangelight" | "ci";
      /**
       * For observer 'ci': the workflow identity from the Sigstore certificate (repo, workflow path, ref).
       */
      workflow?: string;
    },
    {
      id: "tests-pass" | "main-tests-still-pass" | "committed-and-pushed" | "merges-cleanly";
      ok: boolean;
      note: string;
      gateJob?: string;
      commit?: string;
      /**
       * The main SHA the merge-based checks ran against.
       */
      mainCommit?: string;
      /**
       * Who ran the check: Rangelight's own gate Job, or the rangelight/verify GitHub Action in the repo's CI (ADR 0056 §1).
       */
      observer?: "rangelight" | "ci";
      /**
       * For observer 'ci': the workflow identity from the Sigstore certificate (repo, workflow path, ref).
       */
      workflow?: string;
    },
    {
      id: "tests-pass" | "main-tests-still-pass" | "committed-and-pushed" | "merges-cleanly";
      ok: boolean;
      note: string;
      gateJob?: string;
      commit?: string;
      /**
       * The main SHA the merge-based checks ran against.
       */
      mainCommit?: string;
      /**
       * Who ran the check: Rangelight's own gate Job, or the rangelight/verify GitHub Action in the repo's CI (ADR 0056 §1).
       */
      observer?: "rangelight" | "ci";
      /**
       * For observer 'ci': the workflow identity from the Sigstore certificate (repo, workflow path, ref).
       */
      workflow?: string;
    },
    {
      id: "tests-pass" | "main-tests-still-pass" | "committed-and-pushed" | "merges-cleanly";
      ok: boolean;
      note: string;
      gateJob?: string;
      commit?: string;
      /**
       * The main SHA the merge-based checks ran against.
       */
      mainCommit?: string;
      /**
       * Who ran the check: Rangelight's own gate Job, or the rangelight/verify GitHub Action in the repo's CI (ADR 0056 §1).
       */
      observer?: "rangelight" | "ci";
      /**
       * For observer 'ci': the workflow identity from the Sigstore certificate (repo, workflow path, ref).
       */
      workflow?: string;
    },
    ...{
      id: "tests-pass" | "main-tests-still-pass" | "committed-and-pushed" | "merges-cleanly";
      ok: boolean;
      note: string;
      gateJob?: string;
      commit?: string;
      /**
       * The main SHA the merge-based checks ran against.
       */
      mainCommit?: string;
      /**
       * Who ran the check: Rangelight's own gate Job, or the rangelight/verify GitHub Action in the repo's CI (ADR 0056 §1).
       */
      observer?: "rangelight" | "ci";
      /**
       * For observer 'ci': the workflow identity from the Sigstore certificate (repo, workflow path, ref).
       */
      workflow?: string;
    }[]
  ];
  /**
   * The org's own CI results on the pushed head, read from GitHub Check Runs (ADR 0054 §1). Provenance is 'reported': Rangelight saw the report, not the run.
   */
  external?: {
    name: string;
    /**
     * Slug of the GitHub App that published the Check Run; must match rangelight.yaml.
     */
    app: string;
    commit: string;
    conclusion:
      | "success"
      | "failure"
      | "neutral"
      | "cancelled"
      | "skipped"
      | "timed_out"
      | "action_required"
      | "stale"
      | "missing";
    required?: boolean;
    url: string;
    readAt: string;
    provenance?: "reported";
  }[];
  /**
   * Human sign-offs, such as a product manager accepting a preview (ADR 0057 §2). Provenance is 'attested': a person said so; never shown as observed.
   */
  attestations?: {
    kind: "preview-accepted";
    /**
     * The OIDC subject of the person.
     */
    who: string;
    /**
     * The commit the preview was built from.
     */
    commit: string;
    at: string;
    note?: string;
    provenance?: "attested";
  }[];
};

/**
 * in-toto DigestSet, e.g. {"sha256": "..."}.
 *
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "digest".
 */
export interface Digest {
  [k: string]: string;
}
/**
 * A proof artifact with its provenance class (ADR 0041).
 *
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "evidence".
 */
export interface Evidence {
  name: string;
  uri?: string;
  mediaType?: string;
  digest: Digest;
  /**
   * observed: produced by a runner-started gate Job on the committed SHA; claimed: reported by the agent.
   */
  class: "observed" | "claimed";
  /**
   * Gate Job id; required when class is observed.
   */
  gateJob?: string;
  /**
   * The committed SHA the gate ran against.
   */
  commit?: string;
}
/**
 * in-toto DigestSet, e.g. {"sha256": "..."}.
 */
export interface Digest1 {
  [k: string]: string;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "artifact".
 */
export interface Artifact {
  name: string;
  uri?: string;
  mediaType?: string;
  digest: Digest;
}
/**
 * The session's events as an OCI artifact referenced by digest, so the chain verifies offline without API access (ADR 0042).
 */
export interface Artifact1 {
  name: string;
  uri?: string;
  mediaType?: string;
  digest: Digest;
}
