// Repos: the repos Rangelight works in, through useRepos().
import { FolderGit2 } from "lucide-react";
import { ViewHeader } from "@/components/rl/view-header";
import { PreviewMark } from "@/preview/mark";
import { useRepos } from "@/preview/repos";

export function Repos() {
  const { real, sample } = useRepos();
  return (
    <div className="flex h-full min-w-0 flex-col">
      <ViewHeader title="Repositories" sub="What each lane checks out, and the preview it runs." />
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5 max-lg:px-3">
        <section aria-label="Your repos" className="mb-6">
          <h2 className="mb-2 text-[11px] font-semibold tracking-[0.08em] text-dim uppercase">
            Your sessions&apos; repos
          </h2>
          {real.length ? (
            real.map((r) => (
              <p
                key={r.name}
                className="flex items-center gap-2 border-b border-line py-2 text-[13px]"
              >
                <FolderGit2 className="size-4 text-dim" aria-hidden />
                <span className="font-mono text-foreground">{r.name}</span>
                <span className="text-dim">{r.sessions} sessions</span>
              </p>
            ))
          ) : (
            <p className="text-[12.5px] text-dim">No session works in a repo yet.</p>
          )}
        </section>
        <section aria-label="Sample repos">
          <h2 className="mb-2 flex items-center gap-2 text-[11px] font-semibold tracking-[0.08em] text-dim uppercase">
            Preview profiles <PreviewMark />
          </h2>
          {sample.map((r) => (
            <div key={r.name} className="border-b border-line py-2 text-[13px]">
              <span className="font-mono text-foreground">{r.name}</span>{" "}
              <span className="text-dim">@ {r.branch}</span>
              <p className="text-[12.5px] text-t2">{r.profile}</p>
            </div>
          ))}
        </section>
      </div>
    </div>
  );
}
