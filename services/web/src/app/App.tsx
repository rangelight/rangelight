import { SearchX } from "lucide-react";
import { AppLink } from "@/components/rl/app-link";
import { Notice } from "@/components/rl/notice";
import { ClaudeAccess } from "@/features/access/ClaudeAccess";
import { ArchitectHome } from "@/features/architect/ArchitectHome";
import { ForkCompare } from "@/features/fork/ForkCompare";
import { RoleHome } from "@/features/home/RoleHome";
import { LaneView } from "@/features/lane/LaneView";
import { Repos } from "@/features/repos/Repos";
import { RetroCard } from "@/features/retro/RetroCard";
import { Settings } from "@/features/settings/Settings";
import { Coordinator } from "@/features/coordinator/Coordinator";
import { DecisionsInbox } from "@/features/decisions/DecisionsInbox";
import { FleetWall } from "@/features/fleet-wall/FleetWall";
import { Reviews } from "@/features/reviews/Reviews";
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
      {route.page === "architect" && <ArchitectHome />}
      {route.page === "decisions" && <DecisionsInbox />}
      {route.page === "reviews" && <Reviews />}
      {route.page === "coordinator" && <Coordinator />}
      {route.page === "wall" && <FleetWall />}
      {route.page === "home" && <RoleHome />}
      {route.page === "fork" && <ForkCompare />}
      {route.page === "repos" && <Repos />}
      {route.page === "settings" && <Settings />}
      {route.page === "access" && <ClaudeAccess />}
      {route.page === "retro" && <RetroCard />}
      {route.page === "lane" && <LaneView key={route.key} laneKey={route.key} />}
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
