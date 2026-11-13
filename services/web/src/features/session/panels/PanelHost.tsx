// The session's panels beside its terminal: the real feed first, then the mock-up's panels from the
// facade. On a real session each panel is a marked example (an example lane's), its actions off.
import { useState, type ReactNode } from "react";
import { cn } from "@/lib/utils";
import { PanelFrame } from "@/preview/mark";
import {
  CodePanel,
  DiagnosisPanel,
  DiffPanel,
  type PanelProps,
  ReceiptPanel,
  TestsPanel,
} from "./Work";
import { HistoryPanel, LogsPanel, PodPanel, PreviewPanel, ServicesPanel } from "./Infra";

const PANELS = [
  { id: "diff", label: "Diff", Panel: DiffPanel },
  { id: "tests", label: "Tests", Panel: TestsPanel },
  { id: "receipt", label: "Receipt", Panel: ReceiptPanel },
  { id: "diagnosis", label: "Diagnosis", Panel: DiagnosisPanel },
  { id: "pod", label: "Pod", Panel: PodPanel },
  { id: "preview", label: "Preview", Panel: PreviewPanel },
  { id: "code", label: "Code", Panel: CodePanel },
  { id: "logs", label: "Logs", Panel: LogsPanel },
  { id: "services", label: "Services", Panel: ServicesPanel },
  { id: "history", label: "History", Panel: HistoryPanel },
] as const satisfies readonly { id: string; label: string; Panel: (p: PanelProps) => ReactNode }[];

type Shown = "feed" | (typeof PANELS)[number]["id"];

/** `feed`: the session's own feed (real, or a sample lane's); `lane`: the sample lane the panels
 * show; `example`: a real session, whose panels only show what they will show. */
export function PanelHost({
  feed,
  lane,
  example,
}: {
  feed: ReactNode;
  lane: string;
  example: boolean;
}) {
  const [shown, setShown] = useState<Shown>("feed");
  const panel = PANELS.find((p) => p.id === shown);
  const tab = (id: Shown, label: string) => (
    <button
      key={id}
      type="button"
      role="tab"
      aria-selected={shown === id}
      onClick={() => {
        setShown(id);
      }}
      className={cn(
        "shrink-0 border-b-2 px-2.5 py-2 text-[12px] font-medium whitespace-nowrap touch:min-h-11 touch:min-w-11",
        shown === id
          ? "border-rl text-foreground"
          : "border-transparent text-dim hover:text-foreground",
      )}
    >
      {label}
    </button>
  );
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div
        role="tablist"
        aria-label="Session panels"
        data-scroll-x
        className="flex shrink-0 overflow-x-auto border-b border-line px-1"
      >
        {tab("feed", "Events")}
        {PANELS.map((p) => tab(p.id, p.label))}
      </div>
      <div
        role="tabpanel"
        tabIndex={0}
        aria-label={panel?.label ?? "Events"}
        className="flex min-h-0 flex-1 flex-col overflow-y-auto"
      >
        {panel ? (
          <PanelFrame example={example} lane={lane}>
            <panel.Panel lane={lane} acts={!example} />
          </PanelFrame>
        ) : (
          feed
        )}
      </div>
    </div>
  );
}
