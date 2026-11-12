// The view's name, the live stream's status, the signed-in member, search and sign-out.
import { LogOut, PanelLeftOpen, Search } from "lucide-react";
import { useEffect, useState } from "react";
import { auth, signOut } from "@/api/client";
import { useRoute } from "@/app/route";
import { cn } from "@/lib/utils";
import { type SessionsState, useSessions } from "@/features/sessions/store";
import { item } from "./Nav";

const MOD = /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘" : "Ctrl";

/** "Connected" while every session's events stream; "Reconnecting…" while the shell tries again. */
function live(state: SessionsState): string {
  if (state.status === "ready" && !state.paused) return "Connected";
  return state.status === "loading" ? "Connecting…" : "Reconnecting…";
}

/** The member signed in, from WhoAmI; nothing until it answers. */
function useMember(): string {
  const [member, setMember] = useState("");
  useEffect(() => {
    auth.whoAmI({}).then(
      (r) => {
        setMember(r.member);
      },
      () => undefined,
    );
  }, []);
  return member;
}

const plain = "text-dim hover:bg-hover hover:text-foreground";

function Status() {
  const status = live(useSessions().state);
  return (
    <span
      role="status"
      className="flex shrink-0 items-center gap-1.5 text-[12px] text-dim"
      title={status}
    >
      <span
        aria-hidden
        className={cn("size-1.5 rounded-full", status === "Connected" ? "bg-ok" : "bg-warn")}
      />
      <span className="max-sm:sr-only">{status}</span>
    </span>
  );
}

function Member() {
  const member = useMember();
  if (!member) return null;
  return (
    <span
      className="flex shrink-0 items-center gap-1.5 text-[12px] text-t2"
      title={`Signed in as ${member}`}
    >
      <span
        aria-hidden
        className="grid size-6 place-items-center rounded-full bg-surface-2 font-mono text-[11px] uppercase ring-1 ring-line-2"
      >
        {member.slice(0, 1)}
      </span>
      <span className="max-lg:sr-only">
        <span className="sr-only">Signed in as </span>
        {member}
      </span>
    </span>
  );
}

/** The view's name, and the session's key and title on a session page. */
function Current() {
  const route = useRoute();
  const { state } = useSessions();
  const session =
    route.page === "session" && state.status === "ready"
      ? state.sessions.find((s) => s.id === route.id)
      : undefined;
  const view = { sessions: "Sessions", session: "Session", missing: "Not found" }[route.page];
  return (
    <p className="flex min-w-0 flex-1 items-baseline gap-2 text-[12.5px]" aria-label="Current view">
      <span className="shrink-0 font-medium text-foreground">{view}</span>
      {session && (
        <>
          <span className="font-mono text-[12px] text-dim">{session.ticketKey}</span>
          <span className="truncate text-t2 max-sm:hidden">{session.title}</span>
        </>
      )}
    </p>
  );
}

/** Sign out; when the api kept the login, says so here and leaves the button to try again. */
function SignOut() {
  const [failed, setFailed] = useState(false);
  return (
    <>
      {failed && (
        <p
          role="alert"
          className="fixed inset-x-3 top-16 z-10 ml-auto max-w-sm rounded-lg border border-bad/40 bg-background p-3 text-[12.5px] text-foreground shadow-lg"
        >
          <span className="font-semibold text-bad">
            Sign out failed: you&apos;re still signed in.
          </span>{" "}
          Try again in a moment.
        </p>
      )}
      <button
        type="button"
        onClick={() => {
          void signOut().then((ok) => {
            setFailed(!ok);
          });
        }}
        aria-label="Sign out"
        title="Sign out"
        className={cn(
          "flex h-8 shrink-0 items-center gap-1.5 rounded-lg px-2 text-[12px] touch:size-11 touch:justify-center touch:px-0",
          plain,
        )}
      >
        <LogOut className="size-4" aria-hidden />
        <span className="touch:hidden">Sign out</span>
      </button>
    </>
  );
}

export function TopBar({
  fleetCollapsed,
  onFleet,
  onPalette,
}: {
  fleetCollapsed: boolean;
  onFleet: () => void;
  onPalette: () => void;
}) {
  return (
    <div
      data-chrome
      className="flex h-12 shrink-0 items-center gap-2 border-b border-line bg-background px-3 touch:h-14"
    >
      {fleetCollapsed && (
        <button
          type="button"
          onClick={onFleet}
          aria-label="Expand fleet"
          title={`Expand fleet (${MOD} B)`}
          className={cn(item, plain, "max-lg:hidden")}
        >
          <PanelLeftOpen aria-hidden />
        </button>
      )}
      <Current />
      <button
        type="button"
        onClick={onPalette}
        className="flex h-8 items-center gap-2 rounded-lg border border-line bg-card px-2.5 text-[12px] text-faint transition-colors hover:border-line-2 hover:text-dim touch:size-11 touch:justify-center touch:px-0 lg:w-56"
      >
        <Search className="size-3.5" aria-hidden />
        <span className="touch:sr-only">Search</span>
        <kbd aria-hidden className="ml-auto font-mono text-[11px] touch:hidden">
          {MOD} K
        </kbd>
      </button>
      <Status />
      <Member />
      <SignOut />
    </div>
  );
}
