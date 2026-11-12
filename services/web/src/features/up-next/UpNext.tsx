// Up next: the sessions with no lane yet (lanes.ts), oldest first, from the shared WatchSessions stream.
import { timestampMs } from "@bufbuild/protobuf/wkt";
import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { ListTodo } from "lucide-react";
import { signedOut } from "@/api/client";
import { sessionPath } from "@/app/route";
import { AppLink } from "@/components/rl/app-link";
import { Notice } from "@/components/rl/notice";
import { SignedOut } from "@/components/rl/signed-out";
import { PhaseBadge, StateBadge } from "@/components/rl/state-badge";
import { TicketLink } from "@/components/rl/ticket-link";
import { ViewHeader } from "@/components/rl/view-header";
import { notStarted } from "@/features/sessions/lanes";
import { useSessions } from "@/features/sessions/store";
import { elapsed, useNow } from "@/lib/time";

const created = (s: Session): number => (s.createdAt ? timestampMs(s.createdAt) : 0);

function Row({ session: s, n, now }: { session: Session; n: number; now: number }) {
  return (
    <li className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-line px-6 py-3 max-lg:px-4">
      <span className="w-5 shrink-0 font-mono text-[11px] text-faint">{n}</span>
      <span className="flex min-w-0 flex-1 flex-col gap-0.5 max-sm:basis-[calc(100%-2rem)]">
        <AppLink
          href={sessionPath(s.id)}
          className="truncate rounded-sm text-[13px] font-medium text-foreground hover:text-rl touch:flex touch:min-h-11 touch:items-center"
        >
          {s.title || s.ticketKey}
        </AppLink>
        <span className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
          <TicketLink session={s} />
          <span className="font-mono text-[12px] text-t2">{s.repo || "no repo yet"}</span>
        </span>
      </span>
      <span className="flex shrink-0 items-center gap-2 max-sm:pl-8">
        <PhaseBadge phase={s.phase} />
        <StateBadge state={s.state} />
        <span className="font-mono text-[12px] text-dim tabular">{elapsed(created(s), now)}</span>
      </span>
    </li>
  );
}

export function UpNext() {
  const { state } = useSessions();
  const now = useNow();
  if (state.status === "error" && signedOut(state.reason)) return <SignedOut />;
  const waiting =
    state.status === "ready"
      ? state.sessions.filter(notStarted).sort((a, b) => created(a) - created(b))
      : [];
  return (
    <div className="flex h-full min-w-0 flex-col">
      <ViewHeader
        title="Up next"
        sub="The sessions whose lane hasn't started yet, oldest first. Linear stays the system of record."
      />
      <div className="min-h-0 flex-1 overflow-y-auto">
        {state.status === "ready" && !waiting.length && (
          <Notice icon={ListTodo} title="Nothing waiting">
            Every session has a lane. A new labelled ticket shows up here until its lane starts.
          </Notice>
        )}
        {state.status !== "ready" && (
          <p className="px-6 py-6 text-[12.5px] text-dim" role="status">
            Loading…
          </p>
        )}
        <ol aria-label="Up next, oldest first">
          {waiting.map((s, i) => (
            <Row key={s.id} session={s} n={i + 1} now={now} />
          ))}
        </ol>
      </div>
    </div>
  );
}
