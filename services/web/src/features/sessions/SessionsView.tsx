import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { Inbox, PlugZap, RefreshCw } from "lucide-react";
import { signedOut } from "@/api/client";
import { LiveStatus } from "@/components/rl/live-status";
import { Notice } from "@/components/rl/notice";
import { SignedOut } from "@/components/rl/signed-out";
import { ViewHeader } from "@/components/rl/view-header";
import { Button } from "@/components/ui/button";
import { useNow } from "@/lib/time";
import { cn } from "@/lib/utils";
import { SessionRow } from "./SessionRow";
import { useSessions, type SessionsState } from "./store";

export function SessionsView() {
  const { state, reload } = useSessions();
  return (
    <div className="flex h-full min-w-0 flex-col">
      <ViewHeader
        title="Sessions"
        sub="Every Linear ticket labelled rangelight gets a lane. Newest first."
      >
        {state.status === "ready" && <LiveStatus paused={state.paused} onReconnect={reload} />}
      </ViewHeader>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <Body state={state} reload={reload} />
      </div>
    </div>
  );
}

function Body({ state, reload }: { state: SessionsState; reload: () => void }) {
  switch (state.status) {
    case "loading":
      return (
        <p className="px-6 py-6 text-[12.5px] text-dim" role="status">
          Loading sessions…
        </p>
      );
    case "error":
      if (signedOut(state.reason)) return <SignedOut />;
      return (
        <Notice
          icon={PlugZap}
          tone="bad"
          title="Can't reach the Rangelight api"
          action={
            <Button onClick={reload}>
              <RefreshCw aria-hidden /> Try again
            </Button>
          }
        >
          The session list didn't load.{" "}
          <span className="font-mono text-[12px]">{state.reason}</span>
        </Notice>
      );
    case "ready":
      return state.sessions.length ? (
        <Table sessions={state.sessions} />
      ) : (
        <Notice icon={Inbox} title="No sessions yet">
          Label a Linear ticket <span className="font-mono text-[12px] text-t2">rangelight</span>{" "}
          and its lane shows up here within a minute.
        </Notice>
      );
  }
}

function Table({ sessions }: { sessions: Session[] }) {
  const now = useNow();
  return (
    <table className="w-full border-separate max-lg:table-fixed border-spacing-0 text-left">
      <caption className="sr-only">Sessions, newest first</caption>
      <thead className="text-[11px] font-semibold tracking-[0.08em] text-faint uppercase">
        <tr>
          {["Ticket", "Repo", "Phase", "State", "Age"].map((h, i) => (
            <th
              key={h}
              scope="col"
              className={cn(
                i === 4 ? "px-6 py-2.5 text-right" : "px-3 py-2.5 first:pl-6 max-lg:first:pl-4",
                i > 0 && "max-lg:hidden",
              )}
            >
              {h}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {sessions.map((s) => (
          <SessionRow key={s.id} session={s} now={now} />
        ))}
      </tbody>
    </table>
  );
}
