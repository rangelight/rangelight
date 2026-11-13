// Claude access, through useAccess(): each real lane's sign-in state is what its agent asks (real);
// the team's plan and key pool are a preview.
import { sessionPath } from "@/app/route";
import { AppLink } from "@/components/rl/app-link";
import { ViewHeader } from "@/components/rl/view-header";
import { useAccess } from "@/preview/access";
import { PreviewMark } from "@/preview/mark";

export function ClaudeAccess() {
  const { lanes, sample } = useAccess();
  return (
    <div className="flex h-full min-w-0 flex-col">
      <ViewHeader
        title="Claude access"
        sub="How each lane signs in to Claude. Rangelight never stores a Claude credential."
      />
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5 max-lg:px-3">
        <section aria-label="Your lanes" className="mb-6">
          <h2 className="mb-2 text-[11px] font-semibold tracking-[0.08em] text-dim uppercase">
            Your lanes
          </h2>
          {lanes.map((l) => (
            <div key={l.id} className="border-b border-line py-2 text-[13px]">
              <AppLink
                href={sessionPath(l.id)}
                className="font-mono text-foreground hover:underline touch:min-h-11 touch:inline-flex touch:items-center"
              >
                {l.key}
              </AppLink>{" "}
              <span className="text-t2">{l.title}</span>
              <p className={l.question ? "text-warn" : "text-dim"}>
                {l.question ? `Waiting for you: ${l.question}` : "Its agent has no open question."}
              </p>
            </div>
          ))}
        </section>
        <section aria-label="Team access">
          <h2 className="mb-2 flex items-center gap-2 text-[11px] font-semibold tracking-[0.08em] text-dim uppercase">
            Team <PreviewMark />
          </h2>
          {sample.map((s) => (
            <p key={s.label} className="flex gap-3 border-b border-line py-2 text-[13px]">
              <span className="w-24 shrink-0 text-dim">{s.label}</span>
              <span className="text-t2">{s.value}</span>
            </p>
          ))}
        </section>
      </div>
    </div>
  );
}
