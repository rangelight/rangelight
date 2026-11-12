import type { ReactNode } from "react";

export function ViewHeader({
  title,
  sub,
  children,
}: {
  title: ReactNode;
  sub?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <header className="flex shrink-0 flex-wrap items-end gap-4 border-b border-line px-6 pt-5 pb-4 max-lg:px-4 max-lg:pt-4 max-lg:pb-3">
      <div className="min-w-0">
        <h1 className="text-[18px] font-semibold tracking-tight text-foreground">{title}</h1>
        {sub && <div className="mt-1 text-[12.5px] text-dim">{sub}</div>}
      </div>
      <div className="ml-auto flex items-center gap-2">{children}</div>
    </header>
  );
}
