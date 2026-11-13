// The preview facade's types: the mock-up's shapes for what the api doesn't serve yet. Each view's
// hook (preview/<view>.ts) returns these; a real implementation later returns the same types.

/** Whether a value came from the sample world (the view marks it) or from the api. */
export interface Sourced {
  sample: boolean;
}

export interface Person {
  id: string;
  name: string;
  initials: string;
  role: string;
}

export type Tone = "ok" | "warn" | "bad" | "dim" | "brand" | "claude";

export interface DecisionOption {
  key: 1 | 2 | 3;
  label: string;
  meta: string;
  recommended?: boolean;
}

/** A human-in-the-loop moment: a contract trigger, a budget or an access request. */
export interface Decision extends Sourced {
  id: string;
  /** The lane it is about: a sample lane key, or a real session's ticket key. */
  lane: string;
  /** A real session's id, when the decision is one of the api's (its open question). */
  sessionId?: string;
  kind: "escalation" | "budget" | "tool" | "question";
  title: string;
  why: string;
  options: DecisionOption[];
  /** Person id, or "you". */
  approver: string;
  status: "open" | "decided";
  chosen?: 1 | 2 | 3;
  decidedBy?: string;
  at: string;
}

/** One line of a lane's coordinator feed. */
export type FeedEntry =
  | { kind: "outcome"; id: string; text: string; meta?: string; lane?: string; tone?: Tone }
  | { kind: "human"; id: string; who: string; text: string }
  | { kind: "note"; id: string; text: string }
  | { kind: "decision"; id: string; decisionId: string };

/** A pull request a reviewer reads as a story, then as a diff. */
export interface Review extends Sourced {
  /** Unique across real and sample: "pr/<session id>" for a real one, "sample/<n>" for a sample one. */
  id: string;
  /** The pull request's number, as shown. */
  number: string;
  lane: string;
  title: string;
  author: string;
  repos: string;
  proven: string;
  minutes: number;
  chapters: { title: string; body: string }[];
  /** The PR on GitHub, for a real one. */
  url?: string;
  /** Approved in this Workbench (the sample world's only). */
  approved?: boolean;
}

// The types the session panels and the remaining views (RAN-84) put on this facade.

export type StageId = "grill" | "build" | "qa" | "review" | "verify";
export interface Criterion {
  id: string;
  text: string;
  status: "pending" | "running" | "proven" | "failed";
  proof?: { kind: "trace" | "test" | "scan" | "check" | "deploy"; label: string };
}
export interface Contract {
  version: number;
  size: "S" | "M" | "L";
  locked: boolean;
  criteria: Criterion[];
  triggers: string[];
  budget: { usd: number; minutes: number };
}
export interface DiffLine {
  t: "ctx" | "add" | "del";
  n: number;
  text: string;
}
export interface DiffFile {
  repo: string;
  path: string;
  adds: number;
  dels: number;
  hunks: { id: string; header: string; lines: DiffLine[] }[];
}
export interface TestCase {
  id: string;
  gate: "qa" | "sec" | "unit" | "privacy" | "policy";
  suite: string;
  name: string;
  status: "pass" | "fail" | "running" | "skip";
  ms?: number;
}
export interface LogLine {
  t: string;
  src: string;
  level: "info" | "warn" | "error" | "debug";
  text: string;
}
export interface Checkpoint {
  id: string;
  at: string;
  label: string;
  stage: StageId;
  cost: number;
}
