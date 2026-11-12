import { SearchX } from "lucide-react";
import { AppLink } from "@/components/rl/app-link";
import { Notice } from "@/components/rl/notice";
import { FleetWall } from "@/features/fleet-wall/FleetWall";
import { SessionView } from "@/features/session/SessionView";
import { SessionsView } from "@/features/sessions/SessionsView";
import { Shell } from "@/features/shell/Shell";
import { UpNext } from "@/features/up-next/UpNext";
import { useRoute } from "./route";

export function App() {
  const route = useRoute();
  return (
    <Shell>
      {route.page === "sessions" && <SessionsView />}
      {route.page === "wall" && <FleetWall />}
      {route.page === "upnext" && <UpNext />}
      {route.page === "session" && <SessionView key={route.id} id={route.id} />}
      {route.page === "missing" && (
        <Notice icon={SearchX} title="Nothing here">
          This page doesn't exist.{" "}
          <AppLink href="/" className="text-rl underline-offset-4 hover:underline">
            Back to sessions
          </AppLink>
        </Notice>
      )}
    </Shell>
  );
}
