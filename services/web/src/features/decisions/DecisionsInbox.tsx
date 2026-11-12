// The Decisions inbox: every human-in-the-loop moment across the fleet, through useDecisions().
import { Inbox } from "lucide-react";
import type { ReactNode } from "react";
import { Notice } from "@/components/rl/notice";
import { ViewHeader } from "@/components/rl/view-header";
import { useDecisions } from "@/preview/decisions";
import { DecisionCard } from "./DecisionCard";

function Section({
  title,
  count,
  children,
}: {
  title: string;
  count: number;
  children: ReactNode;
}) {
  if (!count) return null;
  return (
    <section aria-label={title} className="mb-8">
      <h2 className="mb-3 text-[11px] font-semibold tracking-[0.08em] text-dim uppercase">
        {title} <span className="font-mono font-normal text-faint">{count}</span>
      </h2>
      <div className="flex flex-col gap-3">{children}</div>
    </section>
  );
}

export function DecisionsInbox() {
  const { needsYou, waiting, decided, decide } = useDecisions();
  const empty = !needsYou.length && !waiting.length && !decided.length;
  return (
    <div className="flex h-full min-w-0 flex-col">
      <ViewHeader
        title="Decisions"
        sub="Every human-in-the-loop moment across your fleet, signed into its ticket's receipt."
      />
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5 max-lg:px-3">
        <div className="mx-auto max-w-[720px]">
          {empty && (
            <Notice icon={Inbox} title="Nothing needs you">
              Lanes stop and ask here when a contract trigger, a budget or a policy says so.
            </Notice>
          )}
          <Section title="Needs you" count={needsYou.length}>
            {needsYou.map((d) => (
              <DecisionCard key={d.id} d={d} decide={decide} />
            ))}
          </Section>
          <Section title="Waiting on others" count={waiting.length}>
            {waiting.map((d) => (
              <DecisionCard key={d.id} d={d} />
            ))}
          </Section>
          <Section title="Decided today" count={decided.length}>
            {decided.map((d) => (
              <DecisionCard key={d.id} d={d} />
            ))}
          </Section>
        </div>
      </div>
    </div>
  );
}
