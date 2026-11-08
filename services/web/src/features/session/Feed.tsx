import { timestampDate } from "@bufbuild/protobuf/wkt";
import type { Change, Event } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { ArrowRightLeft, Boxes, Radio, Sparkles } from "lucide-react";
import type { ReactNode } from "react";
import { phaseLabel, stateLabel } from "@/components/rl/state-badge";

const clock = new Intl.DateTimeFormat(undefined, {
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
});

const mono = "font-mono text-[12px]";

/** The icon and words for one kind of change. */
function said(kind: Change["kind"] | undefined): { icon: ReactNode; text: ReactNode } {
  switch (kind?.case) {
    case "created":
      return {
        icon: <Sparkles className="size-[15px] text-rl" aria-hidden />,
        text: (
          <>
            Session created for <span className={mono}>{kind.value.ticketKey}</span> in{" "}
            <span className={mono}>{kind.value.namespace}</span>,{" "}
            {stateLabel(kind.value.state).toLowerCase()}
          </>
        ),
      };
    case "stateChanged":
      return {
        icon: <ArrowRightLeft className="size-[15px] text-dim" aria-hidden />,
        text: <>State changed to {stateLabel(kind.value.state).toLowerCase()}</>,
      };
    case "phaseChanged":
      return {
        icon: <Boxes className="size-[15px] text-dim" aria-hidden />,
        text: (
          <>
            The runner reports {phaseLabel(kind.value.phase).toLowerCase()}
            {kind.value.message && <span className="text-dim">: {kind.value.message}</span>}
          </>
        ),
      };
    case undefined:
      return {
        icon: <Sparkles className="size-[15px] text-faint" aria-hidden />,
        text: <span className="text-dim">An event this Workbench doesn&apos;t know yet</span>,
      };
  }
}

/** One committed event, rendered per its kind of change. */
function Item({ event }: { event: Event }) {
  const at = event.at ? timestampDate(event.at) : undefined;
  const { icon, text } = said(event.change?.kind);
  return (
    <li className="rl-in flex items-start gap-2.5 rounded-md px-1.5 py-1.5 text-[13px] leading-[1.45]">
      <span className="mt-[3px] shrink-0">{icon}</span>
      <span className="min-w-0 flex-1 text-foreground">{text}</span>
      <span className="shrink-0 font-mono text-[11px] text-faint tabular">
        #{event.seq.toString()}
        {at && <time dateTime={at.toISOString()}> · {clock.format(at)}</time>}
      </span>
    </li>
  );
}

/** The session's events as they commit (WatchSession sends from the next one on). */
export function Feed({ events }: { events: readonly Event[] }) {
  return (
    <section
      aria-labelledby="feed-title"
      className="flex min-h-0 flex-col border-line max-lg:border-b lg:border-r"
    >
      <h2
        id="feed-title"
        className="shrink-0 border-b border-line px-4 py-2.5 text-[12px] font-semibold text-foreground"
      >
        Feed
      </h2>
      {events.length ? (
        <ol aria-live="polite" className="min-h-0 flex-1 overflow-y-auto px-2.5 py-2">
          {events.map((e) => (
            <Item key={e.id} event={e} />
          ))}
        </ol>
      ) : (
        <p className="flex items-center gap-2 px-4 py-4 text-[12.5px] text-dim">
          <Radio className="size-4 shrink-0" aria-hidden />
          Watching. New events appear here as they commit.
        </p>
      )}
    </section>
  );
}
