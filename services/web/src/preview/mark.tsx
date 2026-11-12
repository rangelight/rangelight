import { FlaskConical } from "lucide-react";
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
