// On top of a lane: the newest thing that needs you, real first, and the way back to the Architect.
import { ArrowLeft } from "lucide-react";
import { ARCHITECT, sessionPath } from "@/app/route";
import { AppLink } from "@/components/rl/app-link";
import { cn } from "@/lib/utils";
import { useNeedsYou } from "@/preview/architect";
import { PreviewMark } from "@/preview/mark";

export function ArchitectStrip() {
  const needs = useNeedsYou();
  return (
    <div
      data-strip
      className={cn(
        "flex min-h-8 shrink-0 items-center gap-3 border-b border-line px-3 text-[12px]",
        // An amber edge, not an amber fill: the strip's text and the preview mark keep their contrast.
        needs ? "border-l-2 border-l-warn bg-card" : "bg-rail",
      )}
    >
      <AppLink
        href={ARCHITECT}
        className="flex shrink-0 items-center gap-1 text-dim hover:text-foreground touch:min-h-11"
      >
        <ArrowLeft className="size-3.5" aria-hidden /> Architect
      </AppLink>
      {needs ? (
        <span className="flex min-w-0 items-center gap-2">
          {needs.sessionId ? (
            <AppLink
              href={sessionPath(needs.sessionId)}
              className="truncate text-foreground hover:underline touch:flex touch:min-h-11 touch:items-center"
            >
              <span className="font-medium text-warn">{needs.lane} needs you:</span> {needs.title}
            </AppLink>
          ) : (
            <span className="truncate text-foreground">
              <span className="font-medium text-warn">{needs.lane} needs you:</span> {needs.title}
            </span>
          )}
          {needs.sample && <PreviewMark />}
        </span>
      ) : (
        <span className="text-dim">Nothing needs you.</span>
      )}
    </div>
  );
}
