// The retro card, through useRetro(): a finished sample lane's timeline and the one change it suggests.
import { ViewHeader } from "@/components/rl/view-header";
import { PreviewMark } from "@/preview/mark";
import { useRetro } from "@/preview/retro";

export function RetroCard() {
  const { lane, title, timeline, suggestion } = useRetro();
  const total = timeline.reduce((n, t) => n + t.minutes, 0);
  return (
    <div className="flex h-full min-w-0 flex-col">
      <ViewHeader title="Retro" sub={`${lane} · ${title}`}>
        <PreviewMark />
      </ViewHeader>
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5 max-lg:px-3">
        <ol aria-label="Timeline" className="flex flex-col gap-1.5">
          {timeline.map((t) => (
            <li
              key={t.stage}
              className="grid grid-cols-[70px_1fr_60px] items-center gap-2 text-[12.5px]"
            >
              <span className="text-t2">{t.stage}</span>
              <span className="h-2 rounded-full bg-line">
                <span
                  className="block h-full rounded-full bg-rl"
                  style={{ width: `${String((t.minutes / total) * 100)}%` }}
                />
              </span>
              <span className="text-right font-mono text-[11px] text-dim">{t.minutes} min</span>
            </li>
          ))}
        </ol>
        <p className="mt-5 max-w-prose rounded-lg border border-line bg-card p-3 text-[13px] text-foreground">
          {suggestion}
        </p>
      </div>
    </div>
  );
}
