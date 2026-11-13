import { FlaskConical } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/** The one mark on every view or panel that shows sample data: a member never takes it for real. */
export function PreviewMark({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center gap-1 rounded-md border border-warn/45 bg-warn-soft px-1.5 py-0.5 text-[11px] font-medium whitespace-nowrap text-warn",
        className,
      )}
    >
      <FlaskConical className="size-3" aria-hidden />
      Preview: sample data
    </span>
  );
}

/** How a preview panel is shown: on a sample lane, under the mark; on a real session, as a marked
 * example of what the panel will show, never as that session's own. */
export function PanelFrame({
  example,
  lane,
  children,
}: {
  example: boolean;
  lane: string;
  children: ReactNode;
}) {
  if (!example)
    return (
      <div>
        <div className="px-3 pt-3">
          <PreviewMark />
        </div>
        {children}
      </div>
    );
  return (
    <section
      aria-label="Preview: what this panel will show"
      className="m-3 rounded-xl border border-dashed border-warn/60"
    >
      <p className="flex flex-wrap items-center gap-2 border-b border-dashed border-warn/60 px-3 py-2 text-[12px] text-foreground">
        <PreviewMark />
        What this panel will show: an example from sample lane {lane}, not this session&apos;s.
      </p>
      {children}
    </section>
  );
}
