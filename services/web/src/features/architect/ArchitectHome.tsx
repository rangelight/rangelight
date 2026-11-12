// The Architect home: one stream of what changed across every lane, and what needs you, through
// useArchitect(). The team's architect never edits code; it sequences lanes and asks.
import { CornerUpLeft, Eye, GitMerge, Network, SendHorizontal, Wallet, Zap } from "lucide-react";
import { type ReactNode, useState } from "react";
import { DecisionCard } from "@/features/decisions/DecisionCard";
import { cn } from "@/lib/utils";
import { type ArchitectItem, type ItemKind, useArchitect } from "@/preview/architect";
import { PreviewMark } from "@/preview/mark";
import { notify, SAMPLE_ONLY } from "@/preview/toast";

const MARK: Record<ItemKind, { icon: ReactNode; tone: string; label: string }> = {
  needs: { icon: <Zap className="size-3.5" aria-hidden />, tone: "text-warn", label: "Needs you" },
  acted: { icon: <GitMerge className="size-3.5" aria-hidden />, tone: "text-rl", label: "I acted" },
  heads: { icon: <Eye className="size-3.5" aria-hidden />, tone: "text-dim", label: "Heads-up" },
  waiting: {
    icon: <Wallet className="size-3.5" aria-hidden />,
    tone: "text-dim",
    label: "Waiting on others",
  },
  rollup: {
    icon: <CornerUpLeft className="size-3.5" aria-hidden />,
    tone: "text-dim",
    label: "Since you looked",
  },
};

function Row({
  item,
  focused,
  onFocus,
  undo,
}: {
  item: ArchitectItem;
  focused: boolean;
  onFocus: () => void;
  undo: () => void;
}) {
  return (
    <article
      aria-label={item.title}
      className={cn(
        "rounded-lg border px-3 py-2.5",
        focused ? "border-line-2 bg-raised" : "border-transparent",
      )}
      onClick={onFocus}
    >
      <div className="flex flex-wrap items-center gap-2 text-[11px]">
        <span className={cn("flex items-center gap-1.5 font-medium", MARK[item.kind].tone)}>
          {MARK[item.kind].icon}
          {MARK[item.kind].label}
        </span>
        {item.lanes.map((l) => (
          <span key={l} className="rounded-[5px] border border-line-2 px-1 font-mono text-dim">
            {l}
          </span>
        ))}
        {item.sample && <PreviewMark />}
        <span className="ml-auto font-mono text-faint">{item.at}</span>
      </div>
      <div className="mt-1 text-[13px] break-words text-foreground">{item.title}</div>
      {item.why && <div className="mt-0.5 text-[12.5px] leading-relaxed text-t2">{item.why}</div>}
      {item.id === "overlap" && (
        <button
          type="button"
          onClick={undo}
          className="mt-2 rounded-md border border-line-2 px-2 py-0.5 text-[12px] text-t2 hover:bg-hover touch:min-h-11 touch:px-3"
        >
          Undo
        </button>
      )}
    </article>
  );
}

/** Asking the architect: answered from the sample world, and said so. */
function Ask({ needs }: { needs: ArchitectItem | undefined }) {
  const [draft, setDraft] = useState("");
  const [asked, setAsked] = useState<{ q: string; a: string }[]>([]);
  return (
    <>
      {asked.map((x) => (
        <div key={x.q} className="mt-3 flex flex-col gap-1 px-3 text-[12.5px]">
          <div className="self-end rounded-lg bg-rl-soft px-2.5 py-1.5 text-foreground">{x.q}</div>
          <div className="text-t2">
            {x.a} <PreviewMark />
          </div>
        </div>
      ))}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (!draft.trim()) return;
          const a = needs
            ? `One thing waits on you: ${needs.title}. Everything else is moving.`
            : "Nothing waits on you. Every lane is moving.";
          setAsked((list) => [...list, { q: draft.trim(), a }]);
          setDraft("");
          notify(SAMPLE_ONLY);
        }}
        className="mt-3 flex items-center gap-2 rounded-xl border border-line bg-card px-3 py-2"
      >
        <input
          value={draft}
          onChange={(e) => {
            setDraft(e.target.value);
          }}
          aria-label="Ask the architect"
          placeholder="Ask the architect about any lane…"
          className="min-w-0 flex-1 bg-transparent text-[13px] text-foreground placeholder:text-faint focus:outline-none"
        />
        <button
          type="submit"
          aria-label="Send"
          className="grid size-7 place-items-center rounded-md bg-rl text-primary-foreground touch:size-11"
        >
          <SendHorizontal className="size-3.5" aria-hidden />
        </button>
      </form>
    </>
  );
}

export function ArchitectHome() {
  const { items, watching, undo, decide } = useArchitect();
  const [focusId, setFocus] = useState<string | null>(null);
  const focus =
    items.find((i) => i.id === focusId) ?? items.find((i) => i.kind === "needs") ?? items[0];
  const needs = items.find((i) => i.kind === "needs");
  return (
    <div className="grid h-full min-w-0 lg:grid-cols-[minmax(0,560px)_minmax(0,1fr)]">
      <section aria-label="Architect" className="flex min-h-0 min-w-0 flex-col">
        <div className="flex min-h-10 shrink-0 flex-wrap items-center gap-2 border-b border-line px-3 py-1.5">
          <span className="grid size-6 place-items-center rounded-md bg-claude-soft text-claude">
            <Network className="size-3.5" aria-hidden />
          </span>
          <h1 className="text-[12.5px] font-semibold text-foreground">Architect</h1>
          <span className="text-[12px] text-dim">
            watching {watching.real} lanes, and {watching.sample} sample · never edits code
          </span>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
          <div className="flex flex-col gap-1.5">
            {items.map((i) =>
              i.decision && i.kind === "needs" ? (
                <DecisionCard key={i.id} d={i.decision} decide={decide} />
              ) : (
                <Row
                  key={i.id}
                  item={i}
                  focused={focus?.id === i.id}
                  onFocus={() => {
                    setFocus(i.id);
                  }}
                  undo={undo}
                />
              ),
            )}
          </div>
          <Ask needs={needs} />
        </div>
      </section>
      <aside
        aria-label="Context"
        className="min-w-0 overflow-y-auto border-l border-line bg-card p-4 max-lg:hidden"
      >
        <div className="text-[11px] font-semibold tracking-[0.08em] text-faint uppercase">
          Context
        </div>
        {focus && (
          <>
            <h2 className="mt-1 text-[14px] font-semibold text-foreground">{focus.title}</h2>
            {focus.why && <p className="mt-2 text-[12.5px] leading-relaxed text-t2">{focus.why}</p>}
            {focus.sample && <PreviewMark className="mt-3" />}
          </>
        )}
      </aside>
    </div>
  );
}
