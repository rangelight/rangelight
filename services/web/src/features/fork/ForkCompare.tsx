// Fork compare, through useFork(): a sample lane's two runs side by side, and which to keep.
import { ViewHeader } from "@/components/rl/view-header";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useFork } from "@/preview/fork";
import { PreviewMark } from "@/preview/mark";

export function ForkCompare() {
  const { lane, runs, kept, keep } = useFork();
  return (
    <div className="flex h-full min-w-0 flex-col">
      <ViewHeader
        title="Fork compare"
        sub={`${lane} forked from its latest checkpoint: two approaches, side by side.`}
      >
        <PreviewMark />
      </ViewHeader>
      <div
        tabIndex={0}
        aria-label="The two runs"
        className="grid min-h-0 flex-1 gap-4 overflow-y-auto p-5 max-lg:p-3 lg:grid-cols-2"
      >
        {runs.map((r) => (
          <section
            key={r.id}
            aria-label={r.label}
            className={cn(
              "rounded-xl border bg-card p-4",
              kept === r.id ? "border-ok/60" : "border-line",
            )}
          >
            <h2 className="text-[14px] font-semibold text-foreground">{r.label}</h2>
            <p className="mt-1 text-[12.5px] text-t2">{r.summary}</p>
            <dl className="mt-3 grid grid-cols-2 gap-y-1 font-mono text-[12px] text-t2">
              <dt className="text-dim">cost</dt>
              <dd>${r.cost.toFixed(2)}</dd>
              <dt className="text-dim">time</dt>
              <dd>{r.minutes} min</dd>
              <dt className="text-dim">diff</dt>
              <dd>
                +{r.adds} −{r.dels}
              </dd>
              <dt className="text-dim">criteria</dt>
              <dd>{r.criteria}</dd>
            </dl>
            <Button
              disabled={kept !== null}
              onClick={() => {
                keep(r.id);
              }}
              className="mt-4"
            >
              {kept === r.id ? "Kept" : `Keep ${r.id.toUpperCase()}`}
            </Button>
          </section>
        ))}
      </div>
    </div>
  );
}
