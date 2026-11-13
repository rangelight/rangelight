// A sample lane's session view: the session view's shape (header, full stage strip and budget, the
// feed and the panels), all of it sample data, marked, with its actions live on the sample world.
import { SearchX } from "lucide-react";
import { Notice } from "@/components/rl/notice";
import { ViewHeader } from "@/components/rl/view-header";
import { LaneFeed } from "@/features/coordinator/Coordinator";
import { LaneStrip } from "@/features/session/panels/LaneStrip";
import { PanelHost } from "@/features/session/panels/PanelHost";
import { useCoordinator } from "@/preview/coordinator";
import { PreviewMark } from "@/preview/mark";

export function LaneView({ laneKey }: { laneKey: string }) {
  const { lane, feed, decision, decide } = useCoordinator(laneKey);
  if (lane?.id !== laneKey || !lane.sample)
    return (
      <Notice icon={SearchX} title="No such sample lane">
        Sample lanes are the preview&apos;s ACME-* lanes; a real session has its own page.
      </Notice>
    );
  return (
    <div className="flex h-full min-w-0 flex-col">
      <ViewHeader
        title={lane.title}
        sub={
          <span className="flex flex-wrap items-center gap-2">
            <span className="font-mono text-[12px] text-t2">{lane.key}</span>
            <span>{lane.working}</span>
            <PreviewMark />
          </span>
        }
      />
      <LaneStrip lane={lane.key} example={false} />
      <PanelHost
        lane={lane.key}
        example={false}
        feed={<LaneFeed feed={feed} decision={decision} decide={decide} />}
      />
    </div>
  );
}
