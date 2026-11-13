// The sample world behind every preview hook: one team, its people, lanes, decisions and pull
// requests, coherent across views. engine.ts runs it; nothing here reaches the api.
import type { Decision, FeedEntry, Person, Review, StageId } from "./types";

export type StageState = "todo" | "active" | "paused" | "done" | "skipped";
export const STAGES: { id: StageId; label: string }[] = [
  { id: "grill", label: "Grill" },
  { id: "build", label: "Build" },
  { id: "qa", label: "QA" },
  { id: "review", label: "Review" },
  { id: "verify", label: "Verify" },
];

export const PEOPLE: Record<string, Person> = {
  sam: { id: "sam", name: "Sam Rivera", initials: "SR", role: "Engineer" },
  priya: { id: "priya", name: "Priya Shah", initials: "PS", role: "Engineering manager" },
  jo: { id: "jo", name: "Jo Becker", initials: "JB", role: "Security" },
  lee: { id: "lee", name: "Lee Park", initials: "LP", role: "Product" },
};

/** The sample lanes, by key: what the sample decisions, reviews and notes are about. */
export const LANES = {
  refunds: { key: "ACME-41", title: "Refund credits when a checkout times out" },
  badge: { key: "ACME-42", title: "Cart badge shows a stale count after a merge" },
  rustls: { key: "ACME-44", title: "Upgrade rustls for the latest advisories" },
  webhook: { key: "ACME-45", title: "Webhook retries respect Retry-After" },
} as const;

/** A sample lane: what the sample world's lanes have that the api doesn't serve yet. */
export interface SampleLane {
  key: string;
  title: string;
  repos: string[];
  stage: StageId;
  stages: Record<StageId, StageState>;
  status: "running" | "paused" | "done";
  /** What the lane is doing now. */
  working: string;
  budget: { spent: number; cap: number };
  /** Ticks spent in the current stage. */
  ticks: number;
  feed: FeedEntry[];
}

export interface SampleWorld {
  lanes: SampleLane[];
  decisions: Decision[];
  reviews: Review[];
  /** Whether the architect's sequencing was undone. */
  undone: boolean;
  /** Ids for new entries, so they stay unique. */
  next: number;
}

const decision = (d: Omit<Decision, "sample">): Decision => ({ ...d, sample: true });

/** Where a sample lane starts: its stage, whether it runs, what it does, what it spent. */
interface Start {
  stage: StageId;
  status: SampleLane["status"];
  working: string;
  spent: number;
}

function lane(
  ticket: { key: string; title: string },
  repos: string[],
  start: Start,
  feed: FeedEntry[],
): SampleLane {
  const at = STAGES.findIndex((s) => s.id === start.stage);
  const state = (i: number): StageState =>
    i < at ? "done" : i === at ? (start.status === "paused" ? "paused" : "active") : "todo";
  const stages: Record<StageId, StageState> = {
    grill: state(0),
    build: state(1),
    qa: state(2),
    review: state(3),
    verify: state(4),
  };
  const { stage, status, working, spent } = start;
  return {
    ...ticket,
    repos,
    stage,
    stages,
    status,
    working,
    budget: { spent, cap: 8 },
    ticks: 0,
    feed,
  };
}

const { refunds, badge, rustls } = LANES;

const DECISIONS: Decision[] = [
  decision({
    id: "d-bounce",
    lane: refunds.key,
    kind: "escalation",
    title: "Third QA bounce on refund › timeout after charge",
    why: "The journey expects 12 credits back and gets 13: the refund rounds half up where the finance sheet rounds half to even. The contract says stop after 3 bounces.",
    options: [
      {
        key: 1,
        label: "Round half to even, as the finance sheet says",
        meta: "+1 criterion · ~10 min · $0.60",
        recommended: true,
      },
      {
        key: 2,
        label: "Keep half up and amend the sheet",
        meta: "needs Product · amends the contract",
      },
      {
        key: 3,
        label: "Stop the lane and take it over",
        meta: "hands you the branch at checkpoint 3",
      },
    ],
    approver: "you",
    status: "open",
    at: "11:17",
  }),
  decision({
    id: "d-budget",
    lane: refunds.key,
    kind: "budget",
    title: "Raise the lane's budget from $8 to $12",
    why: "The QA loops spent $4.20 of $8. Finishing with the fix and a fresh QA pass needs about $3 more.",
    options: [
      { key: 1, label: "Raise to $12", meta: "from the team pool", recommended: true },
      { key: 2, label: "Keep $8 and stop at the cap", meta: "the lane pauses" },
    ],
    approver: "priya",
    status: "open",
    at: "11:19",
  }),
  decision({
    id: "d-access",
    lane: badge.key,
    kind: "tool",
    title: "Grant read access to the error tracker",
    why: "The stale badge shows up in production traces; reading them would confirm the merge path.",
    options: [
      { key: 1, label: "Grant read access for this lane", meta: "logged in the receipt" },
      { key: 2, label: "Deny", meta: "the lane reproduces locally instead", recommended: true },
    ],
    approver: "jo",
    status: "open",
    at: "10:44",
  }),
  decision({
    id: "d-merge",
    lane: rustls.key,
    kind: "escalation",
    title: "Merge the dependency bump without a second reviewer",
    why: "Every gate is green and the change only touches the lockfile.",
    options: [
      { key: 1, label: "Merge", meta: "receipt verified · 4/4" },
      { key: 2, label: "Wait for a reviewer", meta: "about 2 h" },
    ],
    approver: "priya",
    status: "decided",
    chosen: 1,
    decidedBy: "priya",
    at: "09:31",
  }),
];

const REVIEWS: Review[] = [
  {
    sample: true,
    id: "sample/212",
    number: "212",
    lane: badge.key,
    title: badge.title,
    author: "sam",
    repos: "storefront · sdk",
    proven: "4/4",
    minutes: 4,
    chapters: [
      {
        title: "What changed and why",
        body: "The cart badge read its count from a cache the merge path never cleared. The merge now clears it in the same transaction, and the badge reads through it.",
      },
      {
        title: "Contract and evidence",
        body: "4 of 4 criteria proven. Two Playwright journeys merge carts across devices and check the badge; a unit test covers the cache key. The receipt's signatures are valid.",
      },
      {
        title: "The parts worth your eyes",
        body: "cart/merge.rs:61 clears the cache inside the transaction; the agent chose this over a TTL after a timing test showed stale reads for up to 4 s.",
      },
      {
        title: "How it was verified",
        body: "Security and privacy gates green. The pod image and seed digests are recorded for a replay.",
      },
    ],
  },
  {
    sample: true,
    id: "sample/210",
    number: "210",
    lane: rustls.key,
    title: rustls.title,
    author: "you",
    repos: "storefront",
    proven: "4/4",
    minutes: 2,
    chapters: [],
  },
];

const START_LANES: SampleLane[] = [
  lane(
    refunds,
    ["storefront", "sdk"],
    { stage: "qa", status: "paused", working: "Waiting on you: third QA bounce", spent: 4.2 },
    [
      {
        kind: "outcome",
        id: "l1",
        text: "Contract locked",
        meta: "5 criteria · 3 triggers · $8 budget",
        tone: "ok",
      },
      {
        kind: "outcome",
        id: "l2",
        text: "Workspace ready in 14s",
        meta: "warm namespace · 6 services",
      },
      {
        kind: "outcome",
        id: "l3",
        lane: "api-lane",
        text: "Refund only what didn't land",
        meta: "+38 −6 · storefront",
      },
      {
        kind: "outcome",
        id: "l4",
        lane: "QA",
        text: "refund › timeout after charge: expected 12 credits, got 13",
        tone: "bad",
      },
      { kind: "decision", id: "l5", decisionId: "d-bounce" },
    ],
  ),
  lane(
    badge,
    ["storefront", "sdk"],
    { stage: "review", status: "running", working: "Resolving 2 review comments", spent: 1.1 },
    [
      {
        kind: "outcome",
        id: "b1",
        text: "Pull request 212 opened with its receipt",
        meta: "4/4 criteria proven",
        tone: "ok",
      },
      { kind: "human", id: "b2", who: "sam", text: "Can the badge refresh without a reload?" },
    ],
  ),
  lane(
    rustls,
    ["storefront"],
    { stage: "verify", status: "running", working: "Waiting for the production sync", spent: 0.6 },
    [
      {
        kind: "outcome",
        id: "r1",
        text: "cargo deny is green on both lockfiles",
        meta: "4 advisories cleared",
        tone: "ok",
      },
      {
        kind: "outcome",
        id: "r2",
        text: "Pull request 210 merged by Priya",
        meta: "receipt verified · 4/4",
        tone: "ok",
      },
    ],
  ),
];

/** The sample world as it starts. */
export function initial(): SampleWorld {
  return {
    undone: false,
    decisions: DECISIONS,
    reviews: REVIEWS,
    lanes: START_LANES,
    next: 1,
  };
}
