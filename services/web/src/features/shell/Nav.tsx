// The Workbench's views: a rail on wider screens, a bottom bar on phones. A view gets an icon here
// when it exists, never before: the views of route.ts's VIEWS, and the session last opened.
import {
  Bot,
  Ellipsis,
  FolderGit2,
  GitPullRequest,
  History,
  House,
  Keyboard,
  LayoutGrid,
  List,
  ListTodo,
  type LucideIcon,
  Moon,
  Network,
  PanelsTopLeft,
  Settings2,
  Signpost,
  SquareTerminal,
  Sun,
} from "lucide-react";
import type { ReactNode } from "react";
import { sessionPath, useRoute, VIEWS } from "@/app/route";
import { AppLink } from "@/components/rl/app-link";
import { cn } from "@/lib/utils";
import { toggleTheme, useTheme } from "@/theme/theme";

export const item =
  "grid size-9 shrink-0 touch:size-11 place-items-center rounded-lg transition-colors duration-150 [&_svg]:size-[18px]";
const idle = "text-dim hover:bg-hover hover:text-foreground";
const here = "bg-raised text-foreground ring-1 ring-line-2";

function ViewLink({
  href,
  label,
  current,
  className,
  children,
}: {
  href: string;
  label: string;
  current: boolean;
  className?: string | undefined;
  children: ReactNode;
}) {
  return (
    <AppLink
      href={href}
      aria-label={label}
      aria-current={current ? "page" : undefined}
      title={label}
      className={cn(item, current ? here : idle, className)}
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

/** Each view's icon. */
const ICONS: Record<Extract<(typeof VIEWS)[number], { rail: true }>["page"], LucideIcon> = {
  home: House,
  fork: History,
  repos: FolderGit2,
  settings: Settings2,
  sessions: PanelsTopLeft,
  architect: Network,
  decisions: Signpost,
  reviews: GitPullRequest,
  coordinator: Bot,
  wall: LayoutGrid,
  upnext: ListTodo,
};

/** `session`: the session last opened, which gets its own icon; `onFleet` opens the fleet sheet
 * where there is no fleet column; `onKeys` the cheat sheet; `onMore` the palette, with every view. */
export function Nav({
  session,
  onFleet,
  onKeys,
  onMore,
}: {
  session: string | null;
  onFleet: () => void;
  onKeys: () => void;
  onMore: () => void;
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
      {VIEWS.filter((v) => v.rail).map((v) => {
        const Icon = ICONS[v.page];
        return (
          <ViewLink
            key={v.page}
            href={v.path}
            label={v.label}
            current={route.page === v.page}
            className={v.phone ? undefined : "max-sm:hidden"}
          >
            <Icon aria-hidden />
          </ViewLink>
        );
      })}
      {session && (
        <ViewLink
          href={sessionPath(session)}
          label="Current session"
          current={route.page === "session"}
        >
          <SquareTerminal aria-hidden />
        </ViewLink>
      )}
      <Action label="More views" onClick={onMore} className="sm:hidden">
        <Ellipsis aria-hidden />
      </Action>
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
