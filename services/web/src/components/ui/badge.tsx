import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

// The mock-up's shadcn badge, with only what the Workbench uses: the outline variant.
const base =
  "group/badge inline-flex h-5 w-fit shrink-0 items-center justify-center gap-1 overflow-hidden rounded-4xl border border-transparent px-2 py-0.5 text-xs font-medium whitespace-nowrap transition-all focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 aria-invalid:border-destructive aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 [&>svg]:pointer-events-none [&>svg]:size-3!";
const outline = "border-border text-foreground [a]:hover:bg-muted [a]:hover:text-muted-foreground";

function Badge({ className, ...props }: ComponentProps<"span">) {
  return (
    <span
      data-slot="badge"
      data-variant="outline"
      className={cn(base, outline, className)}
      {...props}
    />
  );
}

export { Badge };
