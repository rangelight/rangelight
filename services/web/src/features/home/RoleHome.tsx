// Role home, through useHome(): what your seat needs today. Real counts first; the rest is a preview.
import { ARCHITECT, WALL } from "@/app/route";
import { AppLink } from "@/components/rl/app-link";
import { ViewHeader } from "@/components/rl/view-header";
import { useHome } from "@/preview/home";
import { PreviewMark } from "@/preview/mark";

function Tile({
  label,
  value,
  href,
  sample,
}: {
  label: string;
  value: string | number;
  href?: string;
  sample?: boolean;
}) {
  const body = (
    <>
      <span className="block text-[24px] font-semibold text-foreground tabular">{value}</span>
      <span className="flex flex-wrap items-center gap-2 text-[12.5px] text-dim">
        {label} {sample && <PreviewMark />}
      </span>
    </>
  );
  const box = "block rounded-xl border border-line bg-card p-4";
  return href ? (
    <AppLink href={href} className={`${box} hover:border-line-2`}>
      {body}
    </AppLink>
  ) : (
    <div className={box}>{body}</div>
  );
}

export function RoleHome() {
  const { real, sample } = useHome();
  return (
    <div className="flex h-full min-w-0 flex-col">
      <ViewHeader title="Home" sub="What your seat needs today." />
      <div className="grid min-h-0 flex-1 auto-rows-min gap-3 overflow-y-auto p-5 max-lg:p-3 sm:grid-cols-2 lg:grid-cols-3">
        <Tile label="need you now" value={real.needsYou} href={ARCHITECT} />
        <Tile label="lanes running" value={real.live} href={WALL} />
        <Tile label="decisions waiting on others" value={sample.waiting} sample />
        <Tile label="reviews to read" value={sample.reviews} sample />
        <Tile label="given back today" value={sample.timeBack} sample />
      </div>
    </div>
  );
}
