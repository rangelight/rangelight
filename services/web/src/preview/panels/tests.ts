// The Tests panel: a sample lane's gates and journeys, and a re-run of the failing ones.
import { dispatch } from "../engine";
import { notify, SAMPLE_ONLY } from "../toast";
import type { TestCase } from "../types";
import { useLane } from "./lane";

const TESTS: TestCase[] = [
  {
    id: "t1",
    gate: "qa",
    suite: "Journeys",
    name: "refund › timeout after charge",
    status: "fail",
    ms: 8120,
  },
  {
    id: "t2",
    gate: "qa",
    suite: "Journeys",
    name: "refund › error before charge",
    status: "pass",
    ms: 5310,
  },
  {
    id: "t3",
    gate: "qa",
    suite: "Journeys",
    name: "cart › badge after merge",
    status: "pass",
    ms: 4022,
  },
  { id: "t4", gate: "unit", suite: "cargo test", name: "146 tests", status: "pass", ms: 21400 },
  {
    id: "t5",
    gate: "sec",
    suite: "money-path review",
    name: "ledger writes are idempotent",
    status: "pass",
    ms: 41200,
  },
  {
    id: "t6",
    gate: "privacy",
    suite: "privacy grep",
    name: "no URLs, headers or IPs in logs",
    status: "pass",
    ms: 1200,
  },
];

export function useTests(key: string) {
  const lane = useLane(key);
  const tests = TESTS.map((t) =>
    lane?.reran && t.status === "fail" ? { ...t, status: "pass" as const } : t,
  );
  return {
    tests,
    rerun: (): void => {
      if (lane) dispatch({ type: "rerun", lane: lane.key });
      notify(SAMPLE_ONLY);
    },
  };
}
