// The work panels: Diff, Code, Tests, Receipt and Diagnosis, each from its facade hook. `acts` is
// false on a real session: an example's actions never look like they act on the real lane.
import { Check, CircleDashed, RotateCw, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { DecisionCard } from "@/features/decisions/DecisionCard";
import { cn } from "@/lib/utils";
import { useCode } from "@/preview/panels/code";
import { useDiagnosis } from "@/preview/panels/diagnosis";
import { useDiff } from "@/preview/panels/diff";
import { useReceipt } from "@/preview/panels/receipt";
import { useTests } from "@/preview/panels/tests";

export interface PanelProps {
  lane: string;
  acts: boolean;
}

const h3 = "mb-2 text-[11px] font-semibold tracking-[0.08em] text-dim uppercase";
const line = {
  add: "bg-ok-soft text-foreground",
  del: "bg-bad-soft text-foreground",
  ctx: "text-dim",
};

export function DiffPanel({ lane, acts }: PanelProps) {
  const { files, comment } = useDiff(lane);
  return (
    <div className="flex flex-col gap-3 p-3">
      {files.map((f) => (
        <section
          key={f.path}
          aria-label={f.path}
          className="overflow-hidden rounded-lg border border-line"
        >
          <div className="flex flex-wrap items-center gap-2 border-b border-line bg-surface-2 px-3 py-1.5 text-[12px]">
            <span className="font-mono text-t2">
              {f.repo}/{f.path}
            </span>
            <span className="ml-auto font-mono text-ok">+{f.adds}</span>
            <span className="font-mono text-bad">−{f.dels}</span>
          </div>
          {f.hunks.map((h) => (
            <pre
              key={h.id}
              className="overflow-x-auto bg-code py-1 font-mono text-[11.5px] leading-relaxed"
              data-scroll-x
              tabIndex={0}
            >
              <div className="px-3 text-faint">{h.header}</div>
              {h.lines.map((l, i) => (
                <div key={`${String(i)}${l.text}`} className={cn("px-3", line[l.t])}>
                  {l.t === "add" ? "+ " : l.t === "del" ? "- " : "  "}
                  {l.text}
                </div>
              ))}
            </pre>
          ))}
        </section>
      ))}
      <Button disabled={!acts} onClick={comment} className="self-start">
        Comment on the diff
      </Button>
    </div>
  );
}

export function CodePanel({ lane }: PanelProps) {
  const { file } = useCode(lane);
  if (!file) return null;
  return (
    <div className="p-3">
      <div className="mb-2 font-mono text-[12px] text-t2">
        {file.repo}/{file.path}
      </div>
      <pre
        className="overflow-x-auto rounded-lg border border-line bg-code py-2 font-mono text-[11.5px] leading-relaxed"
        data-scroll-x
        tabIndex={0}
      >
        {file.lines.map((l, i) => (
          <div key={`${String(i)}${l}`} className="px-3">
            <span className="mr-3 inline-block w-5 text-right text-faint">{i + 1}</span>
            {l}
          </div>
        ))}
      </pre>
    </div>
  );
}

const STATUS = {
  pass: <Check className="size-3.5 text-ok" aria-label="passed" />,
  fail: <X className="size-3.5 text-bad" aria-label="failed" />,
  running: <CircleDashed className="size-3.5 text-dim" aria-label="running" />,
  skip: <CircleDashed className="size-3.5 text-faint" aria-label="skipped" />,
};

export function TestsPanel({ lane, acts }: PanelProps) {
  const { tests, rerun } = useTests(lane);
  const failing = tests.filter((t) => t.status === "fail").length;
  return (
    <div className="p-3">
      <div className="mb-2 flex flex-wrap items-center gap-2 text-[12.5px]">
        <span className="text-t2">
          {tests.length - failing} of {tests.length} pass
        </span>
        <Button disabled={!acts || !failing} onClick={rerun} className="ml-auto">
          <RotateCw aria-hidden /> Re-run failing
        </Button>
      </div>
      <ul className="flex flex-col">
        {tests.map((t) => (
          <li
            key={t.id}
            className="flex items-center gap-2 border-b border-line py-1.5 text-[12.5px]"
          >
            {STATUS[t.status]}
            <span className="w-14 shrink-0 font-mono text-[11px] text-dim">{t.gate}</span>
            <span className="min-w-0 flex-1 truncate text-foreground">{t.name}</span>
            <span className="font-mono text-[11px] text-faint">
              {((t.ms ?? 0) / 1000).toFixed(1)}s
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

export function ReceiptPanel({ lane }: PanelProps) {
  const { criteria, decisions, manifest } = useReceipt(lane);
  return (
    <div className="flex flex-col gap-5 p-3 text-[12.5px]">
      <section aria-label="Contract">
        <h3 className={h3}>
          Contract · {criteria.filter((c) => c.status === "proven").length}/{criteria.length} proven
        </h3>
        <ol className="flex flex-col gap-1">
          {criteria.map((c, i) => (
            <li key={c.id} className="flex gap-2">
              <span className="w-4 font-mono text-[11px] text-faint">{i + 1}</span>
              <span
                className={cn(
                  "flex-1",
                  c.status === "proven"
                    ? "text-ok"
                    : c.status === "failed"
                      ? "text-bad"
                      : "text-t2",
                )}
              >
                {c.text}
              </span>
              <span className="font-mono text-[11px] text-dim">{c.status}</span>
            </li>
          ))}
        </ol>
      </section>
      <section aria-label="Signed decisions">
        <h3 className={h3}>Signed decisions</h3>
        {decisions.length ? (
          decisions.map((d) => (
            <p key={d.id} className="text-t2">
              {d.title}: option {d.chosen}
            </p>
          ))
        ) : (
          <p className="text-dim">None yet.</p>
        )}
      </section>
      <section aria-label="Manifest">
        <h3 className={h3}>Manifest</h3>
        {manifest.map((m) => (
          <p key={m.name} className="flex gap-2">
            <span className={m.granted ? "text-ok" : "text-bad"}>
              {m.granted ? "granted" : "blocked"}
            </span>
            <span className="text-t2">{m.name}</span>
          </p>
        ))}
      </section>
    </div>
  );
}

export function DiagnosisPanel({ lane, acts }: PanelProps) {
  const { loops, cause, decision, decide } = useDiagnosis(lane);
  return (
    <div className="flex flex-col gap-3 p-3 text-[12.5px]">
      <h3 className={h3}>Why it loops · {loops.length} attempts</h3>
      <ol className="flex flex-col gap-1">
        {loops.map((l) => (
          <li key={l.attempt} className="flex gap-2">
            <span className="font-mono text-[11px] text-faint">{l.at}</span>
            <span className="text-t2">
              {l.attempt}. {l.saw}
            </span>
          </li>
        ))}
      </ol>
      <p className="text-foreground">{cause}</p>
      {decision && <DecisionCard d={decision} {...(acts && { decide })} />}
    </div>
  );
}
