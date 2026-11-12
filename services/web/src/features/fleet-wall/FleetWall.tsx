// The Fleet wall: one tile per live lane (lanes.ts), from the shared WatchSessions stream.
import { timestampMs } from "@bufbuild/protobuf/wkt";
import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { LayoutGrid } from "lucide-react";
import { signedOut } from "@/api/client";
import { sessionPath } from "@/app/route";
import { AppLink } from "@/components/rl/app-link";
import { Notice } from "@/components/rl/notice";
import { SignedOut } from "@/components/rl/signed-out";
import { PhaseBadge, StateBadge } from "@/components/rl/state-badge";
import { ViewHeader } from "@/components/rl/view-header";
import { WaitingBadge } from "@/components/rl/waiting";
import { STAGES, stageOf } from "@/features/session/StageStrip";
import { liveLane } from "@/features/sessions/lanes";
import { useSessions } from "@/features/sessions/store";
import { elapsed, useNow } from "@/lib/time";
import { cn } from "@/lib/utils";

/** What the tile says last: the question if it waits; else its newest event this tab saw (a Session
 * carries no such time), else when the session was created. */
function lastWords(s: Session, lastMs: number | undefined, now: number): string {
  if (s.question) return s.question;
  if (lastMs !== undefined) return `Last activity ${elapsed(lastMs, now)} ago`;
  return s.createdAt ? `Created ${elapsed(timestampMs(s.createdAt), now)} ago` : "";
}

function Tile({
  session: s,
  lastMs,
  now,
}: {
  session: Session;
  lastMs: number | undefined;
  now: number;
}) {
  const at = stageOf(s.phase);
  return (
    <li>
      <AppLink
        href={sessionPath(s.id)}
        className={cn(
          "flex h-full min-h-44 flex-col gap-3 rounded-xl border bg-card p-3.5 transition-colors hover:border-line-2",
          s.question ? "border-warn/60" : "border-line",
        )}
      >
        <span className="flex min-w-0 items-baseline gap-2">
          <span className="shrink-0 font-mono text-[12px] text-dim">{s.ticketKey}</span>
          <span className="truncate text-[13px] font-medium text-foreground">
            {s.title || s.ticketKey}
          </span>
        </span>
        <span className="flex gap-1" aria-hidden>
          {STAGES.map((stage, i) => (
            <span
              key={stage.label}
              title={stage.label}
              className={cn(
                "h-1 flex-1 rounded-full",
                i < at ? "bg-ok" : i === at ? "bg-rl" : "bg-line",
              )}
            />
          ))}
        </span>
        <span className="flex flex-wrap items-center gap-1.5">
          <StateBadge state={s.state} />
          <PhaseBadge phase={s.phase} />
          <WaitingBadge session={s} />
        </span>
        <span
          className={cn("mt-auto line-clamp-2 text-[12px]", s.question ? "text-warn" : "text-dim")}
        >
          {lastWords(s, lastMs, now)}
        </span>
      </AppLink>
    </li>
  );
}

export function FleetWall() {
  const { state, activity } = useSessions();
  const now = useNow();
  if (state.status === "error" && signedOut(state.reason)) return <SignedOut />;
  const lanes = state.status === "ready" ? state.sessions.filter(liveLane) : [];
  return (
    <div className="flex h-full min-w-0 flex-col">
      <ViewHeader
        title="Fleet wall"
        sub="One tile per live lane. A tile turns amber when its lane waits for you."
      />
      <div className="min-h-0 flex-1 overflow-y-auto">
        {state.status === "ready" && !lanes.length && (
          <Notice icon={LayoutGrid} title="No lane is running">
            A tile shows up here once a session&apos;s lane starts.
          </Notice>
        )}
        {state.status !== "ready" && (
          <p className="px-6 py-6 text-[12.5px] text-dim" role="status">
            Loading the fleet…
          </p>
        )}
        <ul className="grid gap-4 p-5 max-sm:p-3 sm:grid-cols-2 2xl:grid-cols-3">
          {lanes.map((s) => (
            <Tile key={s.id} session={s} now={now} lastMs={activity.get(s.id)} />
          ))}
        </ul>
      </div>
    </div>
  );
}
