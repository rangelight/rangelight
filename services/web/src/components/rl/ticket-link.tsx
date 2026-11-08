import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { ExternalLink } from "lucide-react";

/** Only an absolute https URL becomes a link; anything else stays text. */
export const safeHttps = (url: string): boolean =>
  URL.canParse(url) && new URL(url).protocol === "https:";

/** The session's ticket key, linking to its page in the tracker. */
export function TicketLink({ session }: { session: Session }) {
  if (!safeHttps(session.ticketUrl))
    return <span className="font-mono text-[12px] text-dim">{session.ticketKey}</span>;
  return (
    <a
      href={session.ticketUrl}
      target="_blank"
      rel="noopener noreferrer"
      className="inline-flex items-center gap-1 rounded-sm font-mono text-[12px] text-dim hover:text-rl"
    >
      {session.ticketKey}
      <ExternalLink className="size-3" aria-hidden />
      <span className="sr-only"> in Linear (opens in a new tab)</span>
    </a>
  );
}
