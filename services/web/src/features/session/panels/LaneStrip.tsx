// The full stage strip (grill, build, QA, review, verify) and the budget bar, from the facade. On a
// real session it is a marked example of what the strip will show, never that session's.
import { Check, Circle, Pause } from "lucide-react";
import { cn } from "@/lib/utils";
import { PreviewMark } from "@/preview/mark";
import { useBudget } from "@/preview/panels/budget";
import { useStages } from "@/preview/panels/stages";

export function LaneStrip({ lane, example }: { lane: string; example: boolean }) {
  const stages = useStages(lane);
  const budget = useBudget(lane);
  const pct = budget.cap ? Math.min(100, (budget.spent / budget.cap) * 100) : 0;
  return (
    <div
      data-strip
      aria-label={example ? "Preview: what the stage strip will show" : "Stages and budget"}
      role="group"
      className={cn(
        "flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 border-b border-line bg-card px-3 py-1.5 text-[12px]",
        // On a real session it is only an example: a narrow screen gives its rows to the terminal.
        example && "border-dashed border-warn/60 max-lg:hidden",
      )}
    >
      <PreviewMark />
      {example && <span className="text-dim">Example from {lane}:</span>}
      <ol className="flex flex-wrap items-center gap-1">
        {stages.map((s) => (
          <li
            key={s.id}
            aria-current={s.state === "active" || s.state === "paused" ? "step" : undefined}
            className={cn(
              "flex items-center gap-1 rounded-md px-1.5 py-0.5",
              s.state === "active" && "bg-rl-soft text-foreground",
              s.state === "paused" && "bg-warn-soft text-warn",
              s.state === "done" && "text-t2",
              (s.state === "todo" || s.state === "skipped") && "text-dim",
            )}
          >
            {s.state === "done" ? (
              <Check className="size-3 text-ok" aria-hidden />
            ) : s.state === "paused" ? (
              <Pause className="size-3" aria-hidden />
            ) : (
              <Circle className="size-2.5" aria-hidden />
            )}
            {s.label}
          </li>
        ))}
      </ol>
      <span className="ml-auto flex items-center gap-2 font-mono text-t2" title={budget.source}>
        ${budget.spent.toFixed(2)} <span className="text-dim">/ ${budget.cap}</span>
        <span className="relative h-1 w-16 rounded-full bg-line-2" aria-hidden>
          <span
            className="absolute inset-y-0 left-0 rounded-full bg-rl"
            style={{ width: `${String(pct)}%` }}
          />
        </span>
      </span>
    </div>
  );
}
