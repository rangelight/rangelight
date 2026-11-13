// The guided story's bar: the chapter, its caption, and back, play or pause, next and leave. It
// drives the sample world only (preview/story.ts), so it carries the preview mark.
import { Pause, Play, RotateCcw, SkipBack, SkipForward, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PreviewMark } from "@/preview/mark";
import { CHAPTERS, story, useStory } from "@/preview/story";

export function StoryBar() {
  const s = useStory();
  const chapter = CHAPTERS[s.chapter];
  if (!s.on || !chapter) return null;
  const finished = s.done >= chapter.steps.length;
  return (
    <section
      aria-label="Guided story"
      className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-2 border-t border-line bg-card px-3 py-2"
    >
      <div className="flex min-w-0 flex-1 basis-64 items-start gap-2">
        <span className="mt-px font-mono text-[11px] text-faint">
          {s.chapter + 1}/{CHAPTERS.length}
        </span>
        <div className="min-w-0">
          <h2 className="text-[13px] font-semibold text-foreground">{chapter.title}</h2>
          <p className="text-[12.5px] text-dim">{chapter.caption}</p>
        </div>
      </div>
      <div className="flex items-center gap-1.5">
        <PreviewMark />
        <Button aria-label="Previous story chapter" disabled={s.chapter === 0} onClick={story.back}>
          <SkipBack aria-hidden />
        </Button>
        <Button
          aria-label={s.playing ? "Pause" : finished ? "Replay the chapter" : "Play"}
          onClick={s.playing ? story.pause : story.play}
        >
          {s.playing ? (
            <Pause aria-hidden />
          ) : finished ? (
            <RotateCcw aria-hidden />
          ) : (
            <Play aria-hidden />
          )}
        </Button>
        <Button
          aria-label="Next story chapter"
          disabled={s.chapter === CHAPTERS.length - 1}
          onClick={story.next}
        >
          <SkipForward aria-hidden />
        </Button>
        <Button aria-label="Leave the story" onClick={story.leave}>
          <X aria-hidden />
        </Button>
      </div>
    </section>
  );
}
