// The infrastructure panels: Pod, Preview, Services, Logs and History, each from its facade hook.
import { GitFork, Pause, Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useHistory } from "@/preview/panels/history";
import { useLogs } from "@/preview/panels/logs";
import { usePod } from "@/preview/panels/pod";
import { usePreviewPage } from "@/preview/panels/preview-page";
import { useServices } from "@/preview/panels/services";
import type { PanelProps } from "./Work";

const h3 = "mb-2 text-[11px] font-semibold tracking-[0.08em] text-dim uppercase";

function Meter({
  label,
  used,
  limit,
  unit,
}: {
  label: string;
  used: number;
  limit: number;
  unit: string;
}) {
  return (
    <div className="flex items-center gap-2 text-[12px]">
      <span className="w-10 text-dim">{label}</span>
      <span className="h-1.5 flex-1 rounded-full bg-line">
        <span
          className="block h-full rounded-full bg-rl"
          style={{ width: `${String((used / limit) * 100)}%` }}
        />
      </span>
      <span className="font-mono text-[11px] text-t2">
        {used} / {limit} {unit}
      </span>
    </div>
  );
}

export function PodPanel({ lane, acts }: PanelProps) {
  const pod = usePod(lane);
  return (
    <div className="flex flex-col gap-4 p-3 text-[12.5px]">
      <dl className="grid grid-cols-[90px_1fr] gap-y-1 font-mono text-[12px]">
        <dt className="text-dim">namespace</dt>
        <dd className="text-t2">{pod.namespace}</dd>
        <dt className="text-dim">node</dt>
        <dd className="text-t2">{pod.node}</dd>
        <dt className="text-dim">image</dt>
        <dd className="text-t2">{pod.image}</dd>
      </dl>
      <div className="flex flex-col gap-1.5">
        <Meter label="CPU" used={pod.cpu.used} limit={pod.cpu.limit} unit="vCPU" />
        <Meter label="Memory" used={pod.mem.used} limit={pod.mem.limit} unit="GiB" />
      </div>
      <section aria-label="Worktrees">
        <h3 className={h3}>Worktrees</h3>
        {pod.worktrees.map((w) => (
          <p key={w.repo} className="font-mono text-[12px] text-t2">
            {w.repo} · {w.branch} · {w.ahead} ahead
          </p>
        ))}
      </section>
      <Button disabled={!acts} onClick={pod.toggle} className="self-start">
        {pod.paused ? <Play aria-hidden /> : <Pause aria-hidden />}{" "}
        {pod.paused ? "Wake" : "Hibernate"}
      </Button>
    </div>
  );
}

export function PreviewPanel({ lane }: PanelProps) {
  const { url, rows } = usePreviewPage(lane);
  return (
    <div className="p-3">
      <div className="overflow-hidden rounded-lg border border-line">
        <div className="truncate border-b border-line bg-surface-2 px-3 py-1.5 font-mono text-[11.5px] text-dim">
          {url}
        </div>
        <div className="bg-background p-4">
          <h3 className="text-[14px] font-semibold text-foreground">Usage</h3>
          <table className="mt-2 w-full text-left text-[12.5px]">
            <tbody>
              {rows.map((r) => (
                <tr key={r.request} className="border-b border-line">
                  <td className="py-1.5 font-mono text-dim">{r.request}</td>
                  <td className={cn(r.outcome.startsWith("Refunded") ? "text-ok" : "text-t2")}>
                    {r.outcome}
                  </td>
                  <td className="text-right font-mono text-t2">{r.credits}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}

export function ServicesPanel({ lane }: PanelProps) {
  const { services } = useServices(lane);
  return (
    <ul className="flex flex-col p-3">
      {services.map((s) => (
        <li
          key={s.name}
          className="flex flex-wrap items-center gap-2 border-b border-line py-1.5 text-[12.5px]"
        >
          <span
            className={cn("size-1.5 rounded-full", s.health === "Healthy" ? "bg-ok" : "bg-warn")}
            aria-hidden
          />
          <span className="font-mono text-foreground">{s.name}</span>
          <span className="min-w-0 flex-1 truncate text-dim">{s.role}</span>
          <span className="font-mono text-[11px] text-t2">
            {s.health} · {s.source}
          </span>
        </li>
      ))}
    </ul>
  );
}

const LEVEL = { info: "text-dim", warn: "text-warn", error: "text-bad", debug: "text-faint" };

export function LogsPanel({ lane }: PanelProps) {
  const { lines } = useLogs(lane);
  return (
    <pre
      className="m-3 overflow-x-auto rounded-lg border border-line bg-code py-2 font-mono text-[11.5px] leading-relaxed"
      data-scroll-x
      tabIndex={0}
    >
      {lines.map((l) => (
        <div key={l.t + l.text} className="px-3">
          <span className="text-faint">{l.t}</span> <span className={LEVEL[l.level]}>{l.src}</span>{" "}
          <span className="text-foreground">{l.text}</span>
        </div>
      ))}
    </pre>
  );
}

export function HistoryPanel({ lane, acts }: PanelProps) {
  const { checkpoints, fork } = useHistory(lane);
  return (
    <div className="flex flex-col gap-2 p-3 text-[12.5px]">
      <ol className="flex flex-col">
        {checkpoints.map((k) => (
          <li key={k.id} className="flex items-center gap-2 border-b border-line py-1.5">
            <span className="font-mono text-[11px] text-faint">{k.at}</span>
            <span className="flex-1 text-foreground">{k.label}</span>
            <span className="font-mono text-[11px] text-dim">${k.cost.toFixed(2)}</span>
          </li>
        ))}
      </ol>
      <Button disabled={!acts} onClick={fork} className="self-start">
        <GitFork aria-hidden /> Fork from the latest checkpoint
      </Button>
    </div>
  );
}
