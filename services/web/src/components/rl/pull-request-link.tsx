import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { GitPullRequest } from "lucide-react";
import { safeHttps } from "./ticket-link";

/** The session's pull request on GitHub, as #<n>; nothing until the runner opened one. */
export function PullRequestLink({ session }: { session: Session }) {
  if (!safeHttps(session.pullRequestUrl)) return null;
  const number = session.pullRequestUrl.split("/").pop() ?? "";
  return (
    <a
      href={session.pullRequestUrl}
      target="_blank"
      rel="noopener noreferrer"
      className="inline-flex items-center gap-1 rounded-sm font-mono text-[12px] text-dim hover:text-rl"
    >
      <GitPullRequest className="size-3" aria-hidden />#{number}
      <span className="sr-only"> pull request on GitHub (opens in a new tab)</span>
    </a>
  );
}
