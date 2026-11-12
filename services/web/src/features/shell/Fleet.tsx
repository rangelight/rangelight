// The fleet: every session not torn down, with its state and whether it waits for you. A
// column on wide screens, which collapses (remembered in this browser); a sheet on narrow ones.
import { DesiredState } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import { PanelLeftClose } from "lucide-react";
import { type ReactNode, useCallback, useState } from "react";
import { sessionPath, useRoute } from "@/app/route";
import { AppLink } from "@/components/rl/app-link";
import { StateBadge } from "@/components/rl/state-badge";
import { WaitingBadge } from "@/components/rl/waiting";
import { Dialog } from "@/components/ui/dialog";
import { useSessions } from "@/features/sessions/store";
import { cn } from "@/lib/utils";
import { item } from "./Nav";

const KEY = "rl-fleet-collapsed";

/** Whether the fleet column is collapsed, kept in localStorage (best effort: private windows). */
export function useFleetCollapsed(): [boolean, () => void] {
  const [collapsed, setCollapsed] = useState(() => {
    try {
      return localStorage.getItem(KEY) === "1";
    } catch {
      return false;
    }
  });
  const toggle = useCallback((): void => {
    setCollapsed((c) => {
      try {
        localStorage.setItem(KEY, c ? "0" : "1");
      } catch {
        // Not remembered, still toggled.
      }
      return !c;
    });
  }, []);
  return [collapsed, toggle];
}

function List({ onPick }: { onPick?: () => void }) {
  const { state } = useSessions();
  const route = useRoute();
  if (state.status !== "ready")
    return (
      <p className="px-3 py-3 text-[12px] text-dim">
        {state.status === "loading" ? "Loading…" : "Reconnecting…"}
      </p>
    );
  const live = state.sessions.filter((s) => s.state !== DesiredState.COLD);
  if (!live.length) return <p className="px-3 py-3 text-[12px] text-dim">No session is running.</p>;
  return (
    <ul className="flex flex-col gap-0.5 px-1.5 py-2">
      {live.map((s) => {
        const current = route.page === "session" && route.id === s.id;
        return (
          <li key={s.id}>
            <AppLink
              href={sessionPath(s.id)}
              onClick={onPick}
              aria-current={current ? "page" : undefined}
              className={cn(
                "block min-h-11 rounded-lg px-2 py-2 transition-colors",
                current ? "bg-raised ring-1 ring-line-2" : "hover:bg-hover/70",
              )}
            >
              <span className="flex items-center gap-2">
                <span className="font-mono text-[11px] text-dim">{s.ticketKey}</span>
                <span className="ml-auto">
                  <StateBadge state={s.state} />
                </span>
              </span>
              <span className="mt-1 block truncate text-[12.5px] text-t2">
                {s.title || s.ticketKey}
              </span>
              {s.question && (
                <span className="mt-1.5 block">
                  <WaitingBadge session={s} />
                </span>
              )}
            </AppLink>
          </li>
        );
      })}
    </ul>
  );
}

function Title({ children }: { children?: ReactNode }) {
  return (
    <div className="flex h-11 shrink-0 items-center justify-between border-b border-line px-3">
      <h2 className="text-[12px] font-semibold text-foreground">Fleet</h2>
      {children}
    </div>
  );
}

/** The column, at lg and up, unless collapsed. */
export function FleetColumn({
  collapsed,
  onCollapse,
}: {
  collapsed: boolean;
  onCollapse: () => void;
}) {
  if (collapsed) return null;
  return (
    <aside
      aria-label="Fleet"
      className="flex w-64 shrink-0 flex-col border-r border-line bg-background max-lg:hidden"
    >
      <Title>
        <button
          type="button"
          onClick={onCollapse}
          aria-label="Collapse fleet"
          title="Collapse fleet"
          className={cn(item, "size-7 text-dim hover:bg-hover hover:text-foreground")}
        >
          <PanelLeftClose aria-hidden />
        </button>
      </Title>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <List />
      </div>
    </aside>
  );
}

/** The sheet, below lg: the same list, from the bottom; picking a session closes it. */
export function FleetSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Dialog
      open={open}
      onClose={onClose}
      label="Fleet"
      className="mb-0 w-full max-w-none rounded-b-none sm:mb-auto sm:max-w-md sm:rounded-xl"
    >
      <Title />
      <div className="max-h-[70vh] overflow-y-auto">
        <List onPick={onClose} />
      </div>
    </Dialog>
  );
}
