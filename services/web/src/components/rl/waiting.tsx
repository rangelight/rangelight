import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { MessageCircleQuestion } from "lucide-react";
import { Badge } from "@/components/ui/badge";

/** "Waiting for you" while the lane's agent has an open question; nothing otherwise. */
export function WaitingBadge({ session }: { session: Session }) {
  if (!session.question) return null;
  return (
    <Badge
      className="gap-1.5 border-warn/45 bg-warn-soft font-mono text-[11px] text-warn"
      title={session.question}
    >
      <MessageCircleQuestion aria-hidden />
      Waiting for you
      <span className="sr-only">: {session.question}</span>
    </Badge>
  );
}

/** The lane's open question, in full, wrapping: the session view's call to act. */
export function Question({ session }: { session: Session }) {
  if (!session.question) return null;
  return (
    <p role="status" className="mt-2 max-w-prose text-[13px] break-words text-foreground">
      <span className="font-semibold text-warn">Waiting for you: </span>
      {session.question}
    </p>
  );
}
