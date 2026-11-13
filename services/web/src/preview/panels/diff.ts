// The Diff panel: what a sample lane changed, file by file.
import type { DiffFile } from "../types";
import { notify, SAMPLE_ONLY } from "../toast";

const FILES: DiffFile[] = [
  {
    repo: "storefront",
    path: "src/refund.rs",
    adds: 6,
    dels: 2,
    hunks: [
      {
        id: "h1",
        header: "@@ -41,6 +41,10 @@ impl Ledger",
        lines: [
          { t: "ctx", n: 41, text: "impl Ledger {" },
          { t: "del", n: 42, text: "    pub fn refund(&self, req: RequestId) -> Result<()> {" },
          { t: "add", n: 42, text: "    /// Refunds only what didn't land; safe to call twice." },
          {
            t: "add",
            n: 43,
            text: "    pub fn refund_unlanded(&self, req: RequestId, key: IdemKey) -> Result<Refund> {",
          },
          { t: "add", n: 44, text: "        let held = self.hold(req)?;" },
          { t: "add", n: 45, text: "        let landed = self.landed(req)?;" },
          { t: "add", n: 46, text: "        self.write(Refund::new(req, held - landed), key)" },
          { t: "del", n: 43, text: "        self.release_hold(req)" },
          { t: "ctx", n: 47, text: "    }" },
        ],
      },
    ],
  },
  {
    repo: "sdk",
    path: "src/types/usage.ts",
    adds: 2,
    dels: 0,
    hunks: [
      {
        id: "h2",
        header: "@@ -18,3 +18,5 @@ export interface RequestRow",
        lines: [
          { t: "ctx", n: 18, text: "  credits: number" },
          { t: "add", n: 19, text: "  /** Credits returned when the request didn't land. */" },
          { t: "add", n: 20, text: "  creditsRefunded: number" },
        ],
      },
    ],
  },
];

export function useDiff(key: string) {
  return {
    files: key ? FILES : [],
    comment: (): void => {
      notify(SAMPLE_ONLY);
    },
  };
}
