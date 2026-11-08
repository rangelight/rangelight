import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/** A designed empty or error state: never a blank panel. */
export function Notice({
  icon: Icon,
  title,
  tone = "dim",
  children,
  action,
}: {
  icon: LucideIcon;
  title: string;
  tone?: "dim" | "bad";
  children: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div
      role={tone === "bad" ? "alert" : "status"}
      className="rl-in mx-auto flex max-w-md flex-col items-center gap-3 px-6 py-16 text-center"
    >
      <span
        className={cn(
          "grid size-10 place-items-center rounded-xl border",
          tone === "bad"
            ? "border-bad/40 bg-bad-soft text-bad"
            : "border-line bg-surface-2 text-dim",
        )}
      >
        <Icon className="size-5" aria-hidden />
      </span>
      <h2 className="text-[14px] font-semibold text-foreground">{title}</h2>
      <div className="text-[12.5px] leading-relaxed text-dim">{children}</div>
      {action}
    </div>
  );
}
