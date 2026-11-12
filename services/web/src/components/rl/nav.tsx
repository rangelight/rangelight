// The Workbench's two places to go: in the rail on wide screens, in the view header on phones.
import { Moon, PanelsTopLeft, Sun } from "lucide-react";
import { useRoute } from "@/app/route";
import { cn } from "@/lib/utils";
import { toggleTheme, useTheme } from "@/theme/theme";
import { AppLink } from "./app-link";

const item =
  "grid size-9 touch:size-11 place-items-center rounded-lg transition-colors duration-150 [&_svg]:size-[18px]";
const idle = "text-dim hover:bg-hover hover:text-foreground";

export function SessionsLink() {
  const here = useRoute().page === "sessions";
  return (
    <AppLink
      href="/"
      aria-label="Sessions"
      aria-current={here ? "page" : undefined}
      title="Sessions"
      className={cn(item, here ? "bg-raised text-foreground ring-1 ring-line-2" : idle)}
    >
      <PanelsTopLeft aria-hidden />
    </AppLink>
  );
}

export function ThemeToggle({ className }: { className?: string }) {
  const theme = useTheme();
  const label = theme === "dark" ? "Light theme" : "Dark theme";
  return (
    <button
      type="button"
      onClick={toggleTheme}
      aria-label={label}
      title={label}
      className={cn(item, idle, className)}
    >
      {theme === "dark" ? <Sun aria-hidden /> : <Moon aria-hidden />}
    </button>
  );
}
