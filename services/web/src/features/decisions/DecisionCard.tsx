// One human-in-the-loop moment: a real lane's open question (answered in its terminal), or a
// sample decision (decided here, in the sample world only).
import {
  BadgeCheck,
  MessageCircleQuestion,
  PauseCircle,
  ShieldQuestion,
  Wallet,
} from "lucide-react";
import { sessionPath } from "@/app/route";
import { AppLink } from "@/components/rl/app-link";
import { cn } from "@/lib/utils";
import { PreviewMark } from "@/preview/mark";
import type { Decision } from "@/preview/types";
import { PEOPLE } from "@/preview/world";

const KIND = {
  question: { icon: MessageCircleQuestion, label: "Waiting for you" },
  escalation: { icon: PauseCircle, label: "Escalation trigger" },
  budget: { icon: Wallet, label: "Budget sign-off" },
  tool: { icon: ShieldQuestion, label: "Access request" },
} as const;

const who = (id: string): string => (id === "you" ? "you" : (PEOPLE[id]?.name ?? id));

function Decided({ d }: { d: Decision }) {
  const option = d.options.find((o) => o.key === d.chosen);
  return (
    <div className="flex items-start gap-2.5 rounded-lg border border-line bg-card px-3 py-2.5 text-[12.5px]">
      <BadgeCheck className="mt-px size-4 shrink-0 text-ok" aria-hidden />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2 text-foreground">
          {d.title}
          <PreviewMark />
        </div>
        <div className="mt-0.5 text-dim">
          {who(d.decidedBy ?? "you")} chose{" "}
          <span className="text-t2">
            {d.chosen}. {option?.label}
          </span>{" "}
          · signed into the receipt
        </div>
      </div>
    </div>
  );
}

function Options({
  d,
  enabled,
  decide,
}: {
  d: Decision;
  enabled: boolean;
  decide: ((id: string, key: 1 | 2 | 3) => void) | undefined;
}) {
  return (
    <div className="mt-3 flex flex-col gap-1.5" role="group" aria-label="Options">
      {d.options.map((o) => (
        <button
          key={o.key}
          type="button"
          disabled={!enabled}
          onClick={() => decide?.(d.id, o.key)}
          className={cn(
            "flex min-h-11 items-start gap-2.5 rounded-lg border px-2.5 py-2 text-left disabled:cursor-not-allowed disabled:opacity-70",
            o.recommended ? "border-rl/50 bg-rl-soft" : "border-line bg-surface-2 hover:bg-hover",
          )}
        >
          <span className="mt-px grid size-5 shrink-0 place-items-center rounded-[5px] border border-line-2 font-mono text-[11px] text-dim">
            {o.key}
          </span>
          <span className="min-w-0 flex-1">
            <span className="block text-[12.5px] font-medium text-foreground">
              {o.label}
              {o.recommended && <span className="ml-2 font-normal text-rl">Recommended</span>}
            </span>
            <span className="mt-0.5 block font-mono text-[11px] text-dim">{o.meta}</span>
          </span>
        </button>
      ))}
    </div>
  );
}

export function DecisionCard({
  d,
  decide,
}: {
  d: Decision;
  decide?: (id: string, key: 1 | 2 | 3) => void;
}) {
  if (d.status === "decided") return <Decided d={d} />;
  const K = KIND[d.kind];
  const mine = d.approver === "you";
  return (
    <section
      aria-label={d.title}
      className={cn("rounded-xl border bg-card p-3.5", mine ? "border-warn/45" : "border-line-2")}
    >
      <div className="mb-1.5 flex flex-wrap items-center gap-2 text-[12px] text-dim">
        <K.icon className={cn("size-3.5", mine ? "text-warn" : "text-dim")} aria-hidden />
        {K.label}
        <span className="font-mono text-[11px]">{d.lane}</span>
        {d.sample && <PreviewMark />}
        <span className="ml-auto font-mono text-[11px] text-faint">{d.at}</span>
      </div>
      <h3 className="text-[14px] font-semibold break-words text-foreground">{d.title}</h3>
      <p className="mt-1.5 max-w-[68ch] text-[12.5px] leading-relaxed text-t2">{d.why}</p>
      {d.sessionId && (
        <AppLink
          href={sessionPath(d.sessionId)}
          className="mt-2 inline-flex items-center text-[12.5px] font-medium text-rl hover:underline touch:min-h-11"
        >
          Answer in its terminal →
        </AppLink>
      )}
      {d.options.length > 0 && <Options d={d} enabled={mine && !!decide} decide={decide} />}
      <p className="mt-2.5 text-[12px] text-dim">
        Needs <span className="text-t2">{who(d.approver)}</span>
        {!mine && " · they are notified"}
      </p>
    </section>
  );
}
