// The Workbench's views: a rail on wider screens, a bottom bar on phones. A view gets an icon here
// when it exists, never before: today the Sessions list and the session last opened.
import { Keyboard, List, Moon, PanelsTopLeft, SquareTerminal, Sun } from "lucide-react";
import type { ReactNode } from "react";
import { sessionPath, useRoute } from "@/app/route";
import { AppLink } from "@/components/rl/app-link";
import { cn } from "@/lib/utils";
import { toggleTheme, useTheme } from "@/theme/theme";

export const item =
  "grid size-9 shrink-0 touch:size-11 place-items-center rounded-lg transition-colors duration-150 [&_svg]:size-[18px]";
const idle = "text-dim hover:bg-hover hover:text-foreground";
const here = "bg-raised text-foreground ring-1 ring-line-2";

function View({
  href,
  label,
  current,
  children,
}: {
  href: string;
  label: string;
  current: boolean;
  children: ReactNode;
}) {
  return (
    <AppLink
      href={href}
      aria-label={label}
      aria-current={current ? "page" : undefined}
      title={label}
      className={cn(item, current ? here : idle)}
    >
      {children}
    </AppLink>
  );
}

function Action({
  label,
  onClick,
  className,
  children,
}: {
  label: string;
  onClick: () => void;
  className?: string;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className={cn(item, idle, className)}
    >
      {children}
    </button>
  );
}

/** `session`: the session last opened, which gets its own icon; `onFleet` opens the fleet sheet
 * where there is no fleet column; `onKeys` the cheat sheet. */
export function Nav({
  session,
  onFleet,
  onKeys,
}: {
  session: string | null;
  onFleet: () => void;
  onKeys: () => void;
}) {
  const route = useRoute();
  const theme = useTheme();
  const themeLabel = theme === "dark" ? "Light theme" : "Dark theme";
  return (
    <nav
      aria-label="Primary"
      data-chrome
      className="flex shrink-0 gap-1.5 border-line bg-rail max-sm:order-last max-sm:justify-around max-sm:border-t max-sm:px-2 max-sm:py-1 sm:h-full sm:w-[52px] sm:flex-col sm:items-center sm:border-r sm:py-2.5"
    >
      <View href="/" label="Sessions" current={route.page === "sessions"}>
        <PanelsTopLeft aria-hidden />
      </View>
      {session && (
        <View
          href={sessionPath(session)}
          label="Current session"
          current={route.page === "session"}
        >
          <SquareTerminal aria-hidden />
        </View>
      )}
      <Action label="Fleet" onClick={onFleet} className="lg:hidden">
        <List aria-hidden />
      </Action>
      <Action label="Keyboard shortcuts" onClick={onKeys} className="max-sm:hidden sm:mt-auto">
        <Keyboard aria-hidden />
      </Action>
      <Action label={themeLabel} onClick={toggleTheme}>
        {theme === "dark" ? <Sun aria-hidden /> : <Moon aria-hidden />}
      </Action>
    </nav>
  );
}
