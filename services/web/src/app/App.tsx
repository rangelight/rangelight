import { Moon, PanelsTopLeft, SearchX, Sun } from "lucide-react";
import { AppLink } from "@/components/rl/app-link";
import { Notice } from "@/components/rl/notice";
import { SessionView } from "@/features/session/SessionView";
import { SessionsView } from "@/features/sessions/SessionsView";
import { cn } from "@/lib/utils";
import { toggleTheme, useTheme } from "@/theme/theme";
import { useRoute } from "./route";

const railItem =
  "grid size-9 place-items-center rounded-lg transition-colors duration-150 [&_svg]:size-[18px]";

function Rail({ onSessions }: { onSessions: boolean }) {
  const theme = useTheme();
  const label = theme === "dark" ? "Light theme" : "Dark theme";
  return (
    <nav
      aria-label="Primary"
      className="flex h-full w-[52px] shrink-0 flex-col items-center gap-1.5 border-r border-line bg-rail py-2.5"
    >
      <AppLink
        href="/"
        aria-label="Sessions"
        aria-current={onSessions ? "page" : undefined}
        title="Sessions"
        className={cn(
          railItem,
          onSessions
            ? "bg-raised text-foreground ring-1 ring-line-2"
            : "text-dim hover:bg-hover hover:text-foreground",
        )}
      >
        <PanelsTopLeft aria-hidden />
      </AppLink>
      <button
        type="button"
        onClick={toggleTheme}
        aria-label={label}
        title={label}
        className={cn(railItem, "mt-auto text-dim hover:bg-hover hover:text-foreground")}
      >
        {theme === "dark" ? <Sun aria-hidden /> : <Moon aria-hidden />}
      </button>
    </nav>
  );
}

export function App() {
  const route = useRoute();
  return (
    <div className="flex h-full">
      <Rail onSessions={route.page === "sessions"} />
      <main className="min-w-0 flex-1">
        {route.page === "sessions" && <SessionsView />}
        {route.page === "session" && <SessionView key={route.id} id={route.id} />}
        {route.page === "missing" && (
          <Notice icon={SearchX} title="Nothing here">
            This page doesn't exist.{" "}
            <AppLink href="/" className="text-rl underline-offset-4 hover:underline">
              Back to sessions
            </AppLink>
          </Notice>
        )}
      </main>
    </div>
  );
}
