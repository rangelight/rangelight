// The lane's phase as a strip of stages: where its pod is in its life, from the runner's reports.
import { Phase } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import { Check, Circle, X } from "lucide-react";
import { Fragment } from "react";
import { phaseLabel } from "@/components/rl/state-badge";
import { cn } from "@/lib/utils";

/** The stages, each with the phases it stands for; the current one shows the phase's own name. */
export const STAGES: { label: string; phases: Phase[] }[] = [
  { label: "Queued", phases: [Phase.QUEUED] },
  { label: "Starting", phases: [Phase.STARTING] },
  { label: "Running", phases: [Phase.RUNNING] },
  { label: "Hibernated", phases: [Phase.HIBERNATING, Phase.HIBERNATED, Phase.WAKING] },
  { label: "Gone", phases: [Phase.TERMINATING, Phase.GONE] },
];

/** The stage the phase is in, or -1 (not reported, or failed). */
export const stageOf = (phase: Phase): number => STAGES.findIndex((s) => s.phases.includes(phase));

export function StageStrip({ phase }: { phase: Phase }) {
  const at = stageOf(phase);
  const failed = phase === Phase.FAILED;
  return (
    <ol
      aria-label="Lane phase"
      data-strip
      className="flex h-10 shrink-0 items-center gap-1 border-b border-line bg-card px-3 text-[12px] max-sm:px-2"
    >
      {STAGES.map((stage, i) => {
        const current = i === at;
        return (
          <Fragment key={stage.label}>
            {i > 0 && (
              <li
                aria-hidden
                className={cn("h-px w-3 shrink-0", i <= at ? "bg-ok/50" : "bg-line-2")}
              />
            )}
            <li
              aria-current={current ? "step" : undefined}
              className={cn(
                "flex h-7 shrink-0 items-center gap-1.5 rounded-md px-2 font-medium",
                current
                  ? "bg-rl-soft text-foreground ring-1 ring-rl/40"
                  : i < at
                    ? "text-t2"
                    : "text-faint",
              )}
            >
              {i < at ? (
                <Check className="size-3.5 text-ok" aria-hidden />
              ) : (
                <Circle className={cn("size-3", current ? "fill-rl text-rl" : "")} aria-hidden />
              )}
              {/* Phones show only the current stage's name. */}
              <span className={cn("whitespace-nowrap", !current && "max-sm:sr-only")}>
                {current ? phaseLabel(phase) : stage.label}
              </span>
            </li>
          </Fragment>
        );
      })}
      {failed && (
        <li
          aria-current="step"
          className="ml-1 flex h-7 shrink-0 items-center gap-1.5 rounded-md bg-bad-soft px-2 font-medium text-bad ring-1 ring-bad/40"
        >
          <X className="size-3.5" aria-hidden />
          Failed
        </li>
      )}
      {at < 0 && !failed && <li className="ml-auto text-dim">{phaseLabel(phase)}</li>}
    </ol>
  );
}
