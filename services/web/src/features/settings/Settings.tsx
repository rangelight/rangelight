// Settings, through useSettings(): who you are is the api's; the team's settings are a preview.
import { ViewHeader } from "@/components/rl/view-header";
import { Button } from "@/components/ui/button";
import { PreviewMark } from "@/preview/mark";
import { useSettings } from "@/preview/settings";

export function Settings() {
  const { member, sample, save } = useSettings();
  return (
    <div className="flex h-full min-w-0 flex-col">
      <ViewHeader title="Settings" sub="Your account, and your team's defaults." />
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5 max-lg:px-3">
        <dl aria-label="Account" className="mb-6 grid grid-cols-[140px_1fr] gap-y-2 text-[13px]">
          <dt className="text-dim">Signed in as</dt>
          <dd className="text-foreground">{member || "…"}</dd>
        </dl>
        <section aria-label="Team settings">
          <h2 className="mb-2 flex items-center gap-2 text-[11px] font-semibold tracking-[0.08em] text-dim uppercase">
            Team <PreviewMark />
          </h2>
          <dl className="grid grid-cols-[160px_1fr] gap-y-2 text-[13px] max-sm:grid-cols-1">
            {sample.map((s) => (
              <div key={s.label} className="contents">
                <dt className="text-dim">{s.label}</dt>
                <dd className="text-t2">{s.value}</dd>
              </div>
            ))}
          </dl>
          <Button onClick={save} className="mt-4">
            Save the team settings
          </Button>
        </section>
      </div>
    </div>
  );
}
