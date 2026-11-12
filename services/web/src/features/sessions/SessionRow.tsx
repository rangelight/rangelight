import { timestampMs } from "@bufbuild/protobuf/wkt";
import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { sessionPath } from "@/app/route";
import { AppLink } from "@/components/rl/app-link";
import { PullRequestLink } from "@/components/rl/pull-request-link";
import { PhaseBadge, StateBadge } from "@/components/rl/state-badge";
import { TicketLink } from "@/components/rl/ticket-link";
import { elapsed } from "@/lib/time";

const cell = "border-b border-line px-3 py-3 align-middle";
/** The columns a narrow screen folds into the ticket cell. */
const wide = `${cell} max-lg:hidden`;

export function SessionRow({ session, now }: { session: Session; now: number }) {
  const created = session.createdAt ? timestampMs(session.createdAt) : undefined;
  const repo = session.repo || <span className="text-faint">no repo yet</span>;
  const age =
    created === undefined ? (
      "–"
    ) : (
      <time dateTime={new Date(created).toISOString()}>{elapsed(created, now)}</time>
    );
  return (
    <tr className="rl-in hover:bg-hover/60">
      <td className={`${cell} pl-6 max-lg:pl-4`}>
        {/* Narrow: the title on top, then the ticket's and PR's links with the folded columns. */}
        <div className="flex min-w-0 flex-col gap-0.5 max-lg:flex-row max-lg:flex-wrap max-lg:items-center max-lg:gap-x-2.5 max-lg:gap-y-0">
          <span className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-0.5">
            <TicketLink session={session} />
            <PullRequestLink session={session} />
          </span>
          <AppLink
            href={sessionPath(session.id)}
            className="truncate rounded-sm text-[13px] font-medium text-foreground hover:text-rl touch:flex touch:min-h-11 touch:items-center max-lg:order-first max-lg:basis-full"
          >
            {session.title || session.ticketKey}
          </AppLink>
          <div className="contents lg:hidden">
            <span className="min-w-0 truncate font-mono text-[12px] text-t2">{repo}</span>
            <PhaseBadge phase={session.phase} />
            <StateBadge state={session.state} />
            <span className="font-mono text-[12px] text-dim tabular">{age}</span>
          </div>
        </div>
      </td>
      <td className={`${wide} font-mono text-[12px] text-t2`}>{repo}</td>
      <td className={wide}>
        <PhaseBadge phase={session.phase} />
      </td>
      <td className={wide}>
        <StateBadge state={session.state} />
      </td>
      <td className={`${wide} pr-6 text-right font-mono text-[12px] text-dim tabular`}>{age}</td>
    </tr>
  );
}
