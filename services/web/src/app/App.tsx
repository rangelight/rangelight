import { SearchX } from "lucide-react";
import { AppLink } from "@/components/rl/app-link";
import { SessionsLink, ThemeToggle } from "@/components/rl/nav";
import { Notice } from "@/components/rl/notice";
import { SessionView } from "@/features/session/SessionView";
import { SessionsView } from "@/features/sessions/SessionsView";
import { useRoute } from "./route";

/** Wide screens' navigation; on phones its two items move into the view header. */
function Rail() {
  return (
    <nav
      aria-label="Primary"
      className="flex h-full w-[52px] shrink-0 flex-col items-center gap-1.5 border-r border-line bg-rail py-2.5 max-sm:hidden"
    >
      <SessionsLink />
      <ThemeToggle className="mt-auto" />
    </nav>
  );
}

export function App() {
  const route = useRoute();
  return (
    <div className="flex h-full">
      <Rail />
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
