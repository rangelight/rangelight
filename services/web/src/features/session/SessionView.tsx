import { timestampMs } from "@bufbuild/protobuf/wkt";
import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { ArrowLeft, PlugZap, RefreshCw, SearchX } from "lucide-react";
import { type KeyboardEvent, type ReactNode, useState } from "react";
import { signedOut } from "@/api/client";
import { AppLink } from "@/components/rl/app-link";
import { LiveStatus } from "@/components/rl/live-status";
import { Notice } from "@/components/rl/notice";
import { PullRequestLink } from "@/components/rl/pull-request-link";
import { SignedOut } from "@/components/rl/signed-out";
import { PhaseBadge, StateBadge } from "@/components/rl/state-badge";
import { TicketLink } from "@/components/rl/ticket-link";
import { ViewHeader } from "@/components/rl/view-header";
import { Question } from "@/components/rl/waiting";
import { Button } from "@/components/ui/button";
import { elapsed, useNow } from "@/lib/time";
import { cn } from "@/lib/utils";
import { Feed } from "./Feed";
import { StageStrip } from "./StageStrip";
import { useSession } from "./store";
import { Terminal } from "./Terminal";

export function SessionView({ id }: { id: string }) {
  const { state, feed, watch, reconnect } = useSession(id);
  switch (state.status) {
    case "missing":
      return (
        <Notice icon={SearchX} title="No such session" action={<BackLink />}>
          The api has no session with this id. It may have been removed.
        </Notice>
      );
    case "error":
      if (signedOut(state.reason)) return <SignedOut />;
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
    // While it loads, the frame is already there: the panes (the same ones, so the terminal isn't
    // started again once the session comes) under a header that says it is loading.
    case "loading":
    case "ready":
      return (
        // A narrow touch screen with the terminal focused (the soft keyboard up): the header and the
        // stage strip yield their rows to the terminal.
        <div className="flex h-full min-w-0 flex-col max-lg:pointer-coarse:[&:has(.xterm:focus-within)>:is(header,[data-strip])]:hidden">
          {state.status === "ready" ? (
            <>
              <Header session={state.session}>
                <LiveStatus paused={watch === "watching" ? null : watch} onReconnect={reconnect} />
              </Header>
              <StageStrip phase={state.session.phase} />
            </>
          ) : (
            <LoadingHeader />
          )}
          <Panes feed={<Feed events={feed} />} terminal={<Terminal sessionId={id} />} />
        </div>
      );
  }
}

const PANES = { terminal: "Terminal", feed: "Feed" } as const;
type Pane = keyof typeof PANES;
const TABS = ["terminal", "feed"] as const;
/** The ARIA tabs keys: the index each moves to from `i` (wrapping), or none. */
const STEP: Partial<Record<string, (i: number) => number>> = {
  ArrowRight: (i) => i + 1,
  ArrowLeft: (i) => i - 1,
  Home: () => 0,
  End: () => TABS.length - 1,
};

/** Arrows, Home and End move the selection and the focus between the tabs. */
function moveTab(e: KeyboardEvent, shown: Pane, show: (p: Pane) => void): void {
  const step = STEP[e.key];
  if (!step) return;
  e.preventDefault();
  const n = TABS.length;
  const p = TABS[(step(TABS.indexOf(shown)) + n) % n] ?? shown;
  show(p);
  document.getElementById(`tab-${p}`)?.focus();
}

/** Side by side on wide screens; on narrow ones, tabs (both stay mounted, so the terminal keeps its socket). */
function Panes(panes: Record<Pane, ReactNode>) {
  const [shown, setShown] = useState<Pane>("terminal");
  return (
    <>
      <div
        role="tablist"
        aria-label="Session"
        className="flex shrink-0 border-b border-line lg:hidden"
        onKeyDown={(e) => {
          moveTab(e, shown, setShown);
        }}
      >
        {TABS.map((p) => (
          <button
            key={p}
            type="button"
            role="tab"
            id={`tab-${p}`}
            aria-controls={`pane-${p}`}
            aria-selected={shown === p}
            tabIndex={shown === p ? 0 : -1}
            // Focus would leave the terminal first and bring the header back under the finger.
            onPointerDown={(e) => {
              e.preventDefault();
            }}
            onClick={() => {
              setShown(p);
            }}
            className={cn(
              "min-h-11 flex-1 border-b-2 text-[13px] font-medium",
              shown === p ? "border-rl text-foreground" : "border-transparent text-dim",
            )}
          >
            {PANES[p]}
          </button>
        ))}
      </div>
      <div className="flex min-h-0 flex-1 flex-col lg:grid lg:grid-cols-[minmax(320px,2fr)_minmax(0,3fr)]">
        {(["feed", "terminal"] as const).map((p) => (
          <div
            key={p}
            id={`pane-${p}`}
            role="tabpanel"
            aria-labelledby={`tab-${p}`}
            className={cn(
              "min-h-0 flex-1 flex-col *:flex-1 lg:flex",
              shown === p ? "flex" : "hidden",
            )}
          >
            {panes[p]}
          </div>
        ))}
      </div>
    </>
  );
}

function BackLink() {
  return (
    <AppLink
      href="/"
      className="inline-flex items-center gap-1 rounded-sm touch:min-h-11 text-[12px] text-dim hover:text-foreground"
    >
      <ArrowLeft className="size-3.5" aria-hidden /> Sessions
    </AppLink>
  );
}

/** The header's place while the session loads: its back link and a placeholder for the rest. */
function LoadingHeader() {
  return (
    <ViewHeader
      title={<span className="text-dim">Loading the session…</span>}
      sub={
        <span role="status" className="flex flex-wrap items-center gap-x-3 gap-y-1">
          <BackLink />
          <span
            aria-hidden
            className="h-3.5 w-40 max-w-full animate-pulse rounded-sm bg-surface-2"
          />
          <span className="sr-only">Loading the session</span>
        </span>
      }
    />
  );
}

function Header({ session, children }: { session: Session; children: ReactNode }) {
  const now = useNow();
  const created = session.createdAt ? timestampMs(session.createdAt) : undefined;
  return (
    <ViewHeader
      title={session.title || session.ticketKey}
      sub={
        <>
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
          <Question session={session} />
        </>
      }
    >
      {children}
    </ViewHeader>
  );
}
