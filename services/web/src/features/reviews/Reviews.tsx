// Reviews: the story and the receipt first, then the diff, through useReviews(). GitHub stays the
// merge gate.
import {
  BookOpenText,
  Check,
  Clock3,
  ExternalLink,
  GitPullRequest,
  ShieldCheck,
} from "lucide-react";
import { useState } from "react";
import { ViewHeader } from "@/components/rl/view-header";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { PreviewMark } from "@/preview/mark";
import { useReviews } from "@/preview/reviews";
import type { Review } from "@/preview/types";
import { PEOPLE } from "@/preview/world";

/** Whose lane, for a sample review; a real one says only what the api serves (its lane and repo). */
const by = (r: Review): string => {
  if (!r.sample) return "";
  return r.author === "you" ? "Your lane " : `${PEOPLE[r.author]?.name ?? r.author}'s lane `;
};

/** The chapters, as a progress bar to jump through. */
function Chapters({ r, at, setAt }: { r: Review; at: number; setAt: (i: number) => void }) {
  return (
    <ol className="mt-3 flex gap-1">
      {r.chapters.map((c, i) => (
        <li key={c.title} className="flex-1">
          <button
            type="button"
            onClick={() => {
              setAt(i);
            }}
            className="w-full text-left touch:min-h-11"
            aria-current={i === at ? "step" : undefined}
          >
            <span className={cn("block h-1 rounded-full", i <= at ? "bg-rl" : "bg-line")} />
            <span
              className={cn(
                "mt-2 block text-[12px] max-sm:sr-only",
                i === at ? "text-foreground" : "text-dim",
              )}
            >
              {i + 1}. {c.title}
            </span>
          </button>
        </li>
      ))}
    </ol>
  );
}

function Story({ r, approve }: { r: Review; approve: (id: string) => void }) {
  const [at, setAt] = useState(0);
  const chapter = r.chapters[at];
  if (!chapter)
    return (
      <p className="mt-6 text-[13px] text-dim">
        Waiting on a reviewer. Its comments are being resolved by the agent.
      </p>
    );
  return (
    <>
      <div className="mt-5 flex items-center gap-2 text-[11px] font-semibold tracking-[0.08em] text-dim uppercase">
        <BookOpenText className="size-3.5" aria-hidden />
        Story mode · {r.chapters.length} chapters
      </div>
      <Chapters r={r} at={at} setAt={setAt} />
      <h3 className="mt-5 text-[14px] font-semibold text-foreground sm:sr-only">{chapter.title}</h3>
      <article className="mt-3 text-[14.5px] leading-[1.7] text-t2">{chapter.body}</article>
      <div className="mt-8 flex flex-wrap items-center gap-2">
        <Button
          disabled={at === 0}
          onClick={() => {
            setAt(at - 1);
          }}
        >
          Previous
        </Button>
        {at < r.chapters.length - 1 ? (
          <Button
            onClick={() => {
              setAt(at + 1);
            }}
          >
            Next chapter
          </Button>
        ) : (
          <Button
            disabled={r.approved}
            onClick={() => {
              approve(r.id);
            }}
          >
            <Check aria-hidden /> {r.approved ? "Approved" : "Approve"}
          </Button>
        )}
        <span className="ml-auto text-[12px] text-dim">
          Chapter {at + 1} of {r.chapters.length} · ~{r.minutes} min
        </span>
      </div>
    </>
  );
}

/** The pull requests: the real ones (the api's links) first. */
function List({
  reviews,
  open,
  setOpen,
}: {
  reviews: Review[];
  open: Review | undefined;
  setOpen: (id: string) => void;
}) {
  return (
    <ul
      aria-label="Pull requests"
      className="border-line p-2 lg:overflow-y-auto lg:border-r max-lg:border-b"
    >
      {reviews.map((p) => (
        <li key={p.lane + p.id}>
          <button
            type="button"
            onClick={() => {
              setOpen(p.id);
            }}
            aria-current={p.id === open?.id ? "true" : undefined}
            className={cn(
              "w-full rounded-lg px-3 py-2.5 text-left",
              p.id === open?.id ? "bg-raised ring-1 ring-line-2" : "hover:bg-hover/60",
            )}
          >
            <span className="flex flex-wrap items-center gap-2 text-[12px] text-dim">
              <GitPullRequest className="size-3.5 text-ok" aria-hidden />#{p.number} · {p.lane}
              {p.sample && <PreviewMark />}
            </span>
            <span className="mt-1 block text-[13px] text-foreground">{p.title}</span>
            {p.sample && (
              <span className="mt-1.5 flex items-center gap-3 text-[11px] text-dim">
                <span className="flex items-center gap-1 text-ok">
                  <ShieldCheck className="size-3" aria-hidden />
                  {p.proven} proven
                </span>
                <span className="flex items-center gap-1">
                  <Clock3 className="size-3" aria-hidden />~{p.minutes} min to review
                </span>
              </span>
            )}
          </button>
        </li>
      ))}
    </ul>
  );
}

function Detail({ r, approve }: { r: Review; approve: (id: string) => void }) {
  return (
    <section aria-label={r.title} className="min-w-0 lg:overflow-y-auto">
      <div className="mx-auto max-w-[760px] px-8 py-6 max-lg:px-4">
        <span className="text-[12.5px] text-dim">
          {by(r)}
          {r.lane} · {r.repos}
        </span>
        {r.sample && <PreviewMark className="ml-2" />}
        <h2 className="mt-2 text-[20px] font-semibold tracking-tight text-foreground">{r.title}</h2>
        {r.url ? (
          <a
            href={r.url}
            target="_blank"
            rel="noopener noreferrer"
            className="mt-4 inline-flex items-center gap-1.5 text-[13px] text-rl hover:underline touch:min-h-11"
          >
            Open #{r.number} on GitHub <ExternalLink className="size-3.5" aria-hidden />
          </a>
        ) : (
          <Story key={r.id} r={r} approve={approve} />
        )}
      </div>
    </section>
  );
}

export function Reviews() {
  const { reviews, approve } = useReviews();
  const [openId, setOpen] = useState<string | null>(null);
  const r = reviews.find((x) => x.id === openId) ?? reviews[0];
  return (
    <div className="flex h-full min-w-0 flex-col">
      <ViewHeader
        title="Reviews"
        sub="Review the story and the receipt first, then the diff. GitHub stays the merge gate."
      />
      <div className="grid min-h-0 flex-1 overflow-y-auto lg:grid-cols-[320px_minmax(0,1fr)] lg:overflow-hidden">
        <List reviews={reviews} open={r} setOpen={setOpen} />
        {r && <Detail r={r} approve={approve} />}
      </div>
    </div>
  );
}
