// The Receipt panel: a sample lane's contract, the decisions signed into it, and its manifest.
import { useSample } from "../engine";
import type { Criterion } from "../types";
import { useLane } from "./lane";

const CRITERIA: Criterion[] = [
  {
    id: "c1",
    text: "A checkout that times out refunds exactly the held credits",
    status: "proven",
    proof: { kind: "trace", label: "refund › timeout after charge" },
  },
  {
    id: "c2",
    text: "One request is never refunded twice",
    status: "proven",
    proof: { kind: "test", label: "ledger::refund_is_idempotent" },
  },
  {
    id: "c3",
    text: "Refund amounts match the finance sheet",
    status: "failed",
    proof: { kind: "test", label: "ledger::refund_rounding" },
  },
  {
    id: "c4",
    text: "The SDK exposes creditsRefunded with types",
    status: "proven",
    proof: { kind: "check", label: "tsc · sdk" },
  },
  {
    id: "c5",
    text: "No URLs, headers or IPs in logs",
    status: "pending",
    proof: { kind: "scan", label: "privacy grep" },
  },
];

const MANIFEST = [
  { name: "Bash · cargo, pnpm", granted: true },
  { name: "Edit / Write", granted: true },
  { name: "Error tracker (read)", granted: false },
  { name: "Outbound payments API", granted: false },
];

export function useReceipt(key: string) {
  const lane = useLane(key);
  const { decisions } = useSample();
  const criteria = CRITERIA.map((c) =>
    lane?.reran && c.status === "failed" ? { ...c, status: "proven" as const } : c,
  );
  return {
    criteria,
    decisions: decisions.filter((d) => d.lane === lane?.key && d.status === "decided"),
    manifest: MANIFEST,
  };
}
