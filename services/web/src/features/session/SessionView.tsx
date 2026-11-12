import { timestampMs } from "@bufbuild/protobuf/wkt";
import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { ArrowLeft, PlugZap, RefreshCw, SearchX } from "lucide-react";
import type { ReactNode } from "react";
import { AppLink } from "@/components/rl/app-link";
import { LiveStatus } from "@/components/rl/live-status";
import { Notice } from "@/components/rl/notice";
import { PullRequestLink } from "@/components/rl/pull-request-link";
import { PhaseBadge, StateBadge } from "@/components/rl/state-badge";
import { TicketLink } from "@/components/rl/ticket-link";
import { ViewHeader } from "@/components/rl/view-header";
import { Button } from "@/components/ui/button";
import { elapsed, useNow } from "@/lib/time";
import { Feed } from "./Feed";
import { useSession } from "./store";
import { Terminal } from "./Terminal";

export function SessionView({ id }: { id: string }) {
  const { state, feed, watch, reconnect } = useSession(id);
  switch (state.status) {
    case "loading":
      return (
        <p className="px-6 py-6 text-[12.5px] text-dim" role="status">
          Loading the session…
        </p>
      );
    case "missing":
      return (
        <Notice icon={SearchX} title="No such session" action={<BackLink />}>
          The api has no session with this id. It may have been removed.
        </Notice>
      );
    case "error":
      return (
        <Notice
          icon={PlugZap}
          tone="bad"
          title="Can't reach the Rangelight api"
          action={
            <Button onClick={reconnect}>
              <RefreshCw aria-hidden /> Try again
            </Button>
          }
        >
          The session didn't load. <span className="font-mono text-[12px]">{state.reason}</span>
        </Notice>
      );
    case "ready":
      return (
        <div className="flex h-full min-w-0 flex-col">
          <Header session={state.session}>
            <LiveStatus paused={watch === "watching" ? null : watch} onReconnect={reconnect} />
          </Header>
          <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)] grid-rows-2 lg:grid-cols-[minmax(320px,2fr)_minmax(0,3fr)] lg:grid-rows-1">
            <Feed events={feed} />
            <Terminal sessionId={id} />
          </div>
        </div>
      );
  }
}

function BackLink() {
  return (
    <AppLink
      href="/"
      className="inline-flex items-center gap-1 rounded-sm text-[12px] text-dim hover:text-foreground"
    >
      <ArrowLeft className="size-3.5" aria-hidden /> Sessions
    </AppLink>
  );
}

function Header({ session, children }: { session: Session; children: ReactNode }) {
  const now = useNow();
  const created = session.createdAt ? timestampMs(session.createdAt) : undefined;
  return (
    <ViewHeader
      title={session.title || session.ticketKey}
      sub={
        <span className="flex flex-wrap items-center gap-x-3 gap-y-1">
          <BackLink />
          <TicketLink session={session} />
          <PullRequestLink session={session} />
          <span className="font-mono text-[12px] text-t2">{session.repo || "no repo yet"}</span>
          <PhaseBadge phase={session.phase} />
          <StateBadge state={session.state} />
          {created !== undefined && (
            <span className="font-mono text-[12px] tabular">
              started {elapsed(created, now)} ago
            </span>
          )}
        </span>
      }
    >
      {children}
    </ViewHeader>
  );
}
