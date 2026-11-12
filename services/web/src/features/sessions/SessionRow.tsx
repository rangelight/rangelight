import { timestampMs } from "@bufbuild/protobuf/wkt";
import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { sessionPath } from "@/app/route";
import { AppLink } from "@/components/rl/app-link";
import { PullRequestLink } from "@/components/rl/pull-request-link";
import { PhaseBadge, StateBadge } from "@/components/rl/state-badge";
import { TicketLink } from "@/components/rl/ticket-link";
import { elapsed } from "@/lib/time";

const cell = "border-b border-line px-3 py-3 align-middle";

export function SessionRow({ session, now }: { session: Session; now: number }) {
  const created = session.createdAt ? timestampMs(session.createdAt) : undefined;
  return (
    <tr className="rl-in hover:bg-hover/60">
      <td className={`${cell} pl-6`}>
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-0.5">
            <TicketLink session={session} />
            <PullRequestLink session={session} />
          </span>
          <AppLink
            href={sessionPath(session.id)}
            className="truncate rounded-sm text-[13px] font-medium text-foreground hover:text-rl"
          >
            {session.title || session.ticketKey}
          </AppLink>
        </div>
      </td>
      <td className={`${cell} font-mono text-[12px] text-t2`}>
        {session.repo || <span className="text-faint">no repo yet</span>}
      </td>
      <td className={cell}>
        <PhaseBadge phase={session.phase} />
      </td>
      <td className={cell}>
        <StateBadge state={session.state} />
      </td>
      <td className={`${cell} pr-6 text-right font-mono text-[12px] text-dim tabular`}>
        {created === undefined ? (
          "–"
        ) : (
          <time dateTime={new Date(created).toISOString()}>{elapsed(created, now)}</time>
        )}
      </td>
    </tr>
  );
}
