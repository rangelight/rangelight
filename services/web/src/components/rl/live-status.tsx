import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

const PAUSED = {
  missed: "Events were missed",
  stopped: "Live updates stopped",
} as const;

/** "Live" while events stream; otherwise why they stopped, and a way to resume. */
export function LiveStatus({
  paused,
  onReconnect,
}: {
  paused: keyof typeof PAUSED | null;
  onReconnect: () => void;
}) {
  // One status element whose text changes, so a screen reader hears each change.
  return (
    <span
      className={cn(
        "flex items-center text-[12px]",
        paused ? "gap-2 text-warn" : "gap-1.5 text-dim",
      )}
    >
      {!paused && <span className="rl-pulse size-1.5 rounded-full bg-ok" aria-hidden />}
      <span role="status">{paused ? PAUSED[paused] : "Live"}</span>
      {paused && (
        <Button onClick={onReconnect}>
          <RefreshCw aria-hidden /> Reconnect
        </Button>
      )}
    </span>
  );
}
