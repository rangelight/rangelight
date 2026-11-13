// The Logs panel: a sample lane's log lines, newest last.
import type { LogLine } from "../types";

const LOGS: LogLine[] = [
  {
    t: "09:12:04",
    src: "rangelight",
    level: "info",
    text: "lane scheduled · warm namespace claimed (1.8s)",
  },
  { t: "09:12:18", src: "agent", level: "info", text: "agent ready · worktrees: storefront, sdk" },
  {
    t: "10:41:02",
    src: "netpol",
    level: "warn",
    text: "egress denied agent → payments API · fell back to payments-mock",
  },
  {
    t: "10:53:11",
    src: "qa",
    level: "error",
    text: "refund › timeout after charge · expected 12 credits, got 13",
  },
  {
    t: "11:17:30",
    src: "qa",
    level: "error",
    text: "refund › timeout after charge · expected 12 credits, got 13",
  },
  {
    t: "11:17:31",
    src: "rangelight",
    level: "warn",
    text: "trigger fired: 3 QA bounces on the same journey",
  },
];

export function useLogs(key: string) {
  return { lines: key ? LOGS : [] };
}
