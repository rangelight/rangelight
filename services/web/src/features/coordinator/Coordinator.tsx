// The Coordinator: a lane's agent feed, its decision cards and a composer, through useCoordinator().
// A real session's header and open question are the api's; its feed here is a preview.
import { ArrowUp, GitFork, Pause, Play } from "lucide-react";
import { useState } from "react";
import { lanePath, sessionPath } from "@/app/route";
import { AppLink } from "@/components/rl/app-link";
import { ViewHeader } from "@/components/rl/view-header";
import { Button } from "@/components/ui/button";
import { DecisionCard } from "@/features/decisions/DecisionCard";
import { cn } from "@/lib/utils";
import { type CoordinatorLane, useCoordinator } from "@/preview/coordinator";
import { PreviewMark } from "@/preview/mark";
import type { Decision, FeedEntry, Tone } from "@/preview/types";
import { PEOPLE } from "@/preview/world";

const TONE: Record<Tone, string> = {
  ok: "text-ok",
  warn: "text-warn",
  bad: "text-bad",
  dim: "text-dim",
  brand: "text-rl",
  claude: "text-claude",
};

function Entry({
  e,
  decision,
  decide,
}: {
  e: FeedEntry;
  decision: (id: string) => Decision | undefined;
  decide: (id: string, key: 1 | 2 | 3) => void;
}) {
  switch (e.kind) {
    case "outcome":
      return (
        <li className="flex items-start gap-2.5 px-1.5 py-1.5 text-[13px] leading-[1.45]">
          <span
            className={cn(
              "mt-[7px] size-1.5 shrink-0 rounded-full bg-current",
              e.tone ? TONE[e.tone] : "text-dim",
            )}
            aria-hidden
          />
          <span className="min-w-0 flex-1">
            <span className={e.tone === "bad" ? "text-bad" : "text-foreground"}>{e.text}</span>
            {e.meta && <span className="ml-2 text-[12px] text-dim">{e.meta}</span>}
          </span>
          {e.lane && (
            <span className="shrink-0 rounded-[4px] border border-line px-1.5 font-mono text-[11px] text-dim">
              {e.lane}
            </span>
          )}
          <PreviewMark />
        </li>
      );
    case "human":
      return (
        <li className="my-1.5 rounded-lg bg-surface-2 px-2.5 py-2 text-[13px]">
          <span className="mr-1.5 text-[12px] font-medium text-dim">
            {e.who === "you" ? "You" : (PEOPLE[e.who]?.name ?? e.who)}
          </span>
          <span className="text-foreground">{e.text}</span> <PreviewMark />
        </li>
      );
    case "note":
      return (
        <li className="my-1 px-1.5 py-1 text-[13px] text-t2">
          {e.text} <PreviewMark />
        </li>
      );
    case "decision": {
      const d = decision(e.decisionId);
      return d ? (
        <li className="my-2">
          <DecisionCard d={d} decide={decide} />
        </li>
      ) : null;
    }
  }
}

/** A sample lane's feed, as a list: each entry marked, its decisions decidable. */
export function LaneFeed({
  feed,
  decision,
  decide,
}: {
  feed: FeedEntry[];
  decision: (id: string) => Decision | undefined;
  decide: (id: string, key: 1 | 2 | 3) => void;
}) {
  return (
    <ol aria-label="Feed" className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
      {feed.map((e) => (
        <Entry key={e.id} e={e} decision={decision} decide={decide} />
      ))}
    </ol>
  );
}

/** The composer: a sample lane's, which answers from the sample world; on a real lane, disabled,
 * pointing at its terminal, where a word really reaches it. */
function Composer({ lane, say }: { lane: CoordinatorLane; say: (text: string) => void }) {
  const [text, setText] = useState("");
  const send = (): void => {
    if (!text.trim()) return;
    say(text.trim());
    setText("");
  };
  return (
    <div className="shrink-0 border-t border-line p-3">
      {!lane.sample && (
        <AppLink
          href={sessionPath(lane.id)}
          className="mb-2 flex items-center text-[12.5px] font-medium text-rl hover:underline touch:min-h-11"
        >
          Talk to this lane in its terminal →
        </AppLink>
      )}
      <div className="flex items-end gap-2">
        <textarea
          value={text}
          rows={2}
          disabled={!lane.sample}
          aria-label="Message the coordinator"
          placeholder={lane.sample ? "Ask the coordinator…" : "Not here: this is a real lane"}
          onChange={(e) => {
            setText(e.target.value);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
          className="min-w-0 flex-1 resize-none rounded-lg border border-line-2 bg-card px-3 py-2 text-[13px] text-foreground placeholder:text-dim focus:outline-none disabled:opacity-60"
        />
        <button
          type="button"
          onClick={send}
          disabled={!lane.sample || !text.trim()}
          aria-label="Send"
          className="grid size-9 place-items-center rounded-lg bg-rl text-primary-foreground disabled:opacity-40 touch:size-11"
        >
          <ArrowUp className="size-4" aria-hidden />
        </button>
      </div>
    </div>
  );
}

/** The lane: its key and title, what it does, and (a sample lane) pause, resume and fork. */
function LaneBar({
  lane,
  pause,
  fork,
}: {
  lane: CoordinatorLane;
  pause: () => void;
  fork: () => void;
}) {
  return (
    <div className="flex min-h-10 shrink-0 flex-wrap items-center gap-2 border-b border-line px-3 py-1.5 text-[12.5px]">
      <span className="font-mono text-dim">{lane.key}</span>
      <span className="truncate font-medium text-foreground">{lane.title}</span>
      {lane.working && <span className="text-dim">· {lane.working}</span>}
      {lane.sample && <PreviewMark className="ml-auto" />}
      {lane.sample && (
        <AppLink
          href={lanePath(lane.id)}
          className="text-[12px] text-rl hover:underline touch:flex touch:min-h-11 touch:items-center"
        >
          Open its session view
        </AppLink>
      )}
      {lane.sample && lane.status !== "done" && (
        <>
          <Button onClick={pause}>
            {lane.status === "paused" ? (
              <>
                <Play aria-hidden /> Resume
              </>
            ) : (
              <>
                <Pause aria-hidden /> Pause
              </>
            )}
          </Button>
          <Button onClick={fork}>
            <GitFork aria-hidden /> Fork
          </Button>
        </>
      )}
    </div>
  );
}

export function Coordinator() {
  const [selected, setSelected] = useState<string | null>(null);
  const { lanes, lane, feed, decision, say, decide, pause, fork } = useCoordinator(selected);
  return (
    <div className="flex h-full min-w-0 flex-col">
      <ViewHeader
        title="Coordinator"
        sub="A lane's agent: what it did, what it asks, and a word from you."
      >
        <select
          aria-label="Lane"
          value={lane?.id ?? ""}
          onChange={(e) => {
            setSelected(e.target.value);
          }}
          className="h-8 max-w-56 rounded-lg border border-line bg-card px-2 text-[12.5px] text-foreground touch:h-11"
        >
          <optgroup label="Your sessions">
            {lanes.real.map((l) => (
              <option key={l.id} value={l.id}>
                {l.key} · {l.title}
              </option>
            ))}
          </optgroup>
          <optgroup label="Sample lanes">
            {lanes.sample.map((l) => (
              <option key={l.id} value={l.id}>
                {l.key} · {l.title}
              </option>
            ))}
          </optgroup>
        </select>
      </ViewHeader>
      {lane && (
        <section aria-label={`${lane.key} coordinator`} className="flex min-h-0 flex-1 flex-col">
          <LaneBar lane={lane} pause={pause} fork={fork} />
          <ol aria-label="Feed" className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
            {lane.ask && (
              <li className="mb-2">
                <DecisionCard d={lane.ask} />
              </li>
            )}
            {!lane.sample && (
              <li className="px-1.5 py-1 text-[12.5px] text-dim">
                This lane&apos;s events and terminal are on its session page.
              </li>
            )}
            {feed.map((e) => (
              <Entry key={e.id} e={e} decision={decision} decide={decide} />
            ))}
          </ol>
          <Composer lane={lane} say={say} />
        </section>
      )}
    </div>
  );
}
