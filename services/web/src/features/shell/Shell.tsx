// The mock-up's app shell around the Workbench's views: the rail (a bottom bar on phones), the top
// bar, the fleet (a column, or a sheet on narrow screens), the command palette and the cheat sheet.
import { type ReactNode, useEffect, useState } from "react";
import { useRoute } from "@/app/route";
import { SessionsProvider } from "@/features/sessions/store";
import { FleetColumn, FleetSheet, useFleetCollapsed } from "./Fleet";
import { Nav } from "./Nav";
import { CheatSheet, Palette } from "./Palette";
import { TopBar } from "./TopBar";
import { Toaster } from "@/preview/toast";

type Overlay = "fleet" | "palette" | "keys" | null;

/** Where a key typed now goes to a field, or to the terminal, rather than to the shell. */
const typing = (e: KeyboardEvent): boolean =>
  e.target instanceof Element &&
  e.target.closest("input, textarea, select, [contenteditable], .xterm") !== null;

/** Ctrl/⌘ K, Ctrl/⌘ B and ?, as the cheat sheet (`KEYS`) lists them. */
function useKeys(toggleFleet: () => void, setOverlay: (o: (now: Overlay) => Overlay) => void) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.defaultPrevented || e.isComposing) return;
      const mod = (e.metaKey || e.ctrlKey) && !e.altKey;
      // In the terminal, Ctrl+K and Ctrl+B are the shell's own (kill line, back a character).
      const terminal = e.target instanceof Element && e.target.closest(".xterm") !== null;
      if (mod && e.key.toLowerCase() === "k" && !terminal) {
        e.preventDefault();
        setOverlay((o) => (o === "palette" ? null : "palette"));
      } else if (mod && e.key.toLowerCase() === "b" && !typing(e)) {
        e.preventDefault();
        toggleFleet();
      } else if (e.key === "?" && !mod && !typing(e)) {
        e.preventDefault();
        setOverlay(() => "keys");
      }
    };
    addEventListener("keydown", onKey);
    return () => {
      removeEventListener("keydown", onKey);
    };
  }, [toggleFleet, setOverlay]);
}

/** The session last opened: the rail's second view. */
function useLastSession(): string | null {
  const route = useRoute();
  const [last, setLast] = useState<string | null>(null);
  const id = route.page === "session" ? route.id : null;
  useEffect(() => {
    if (id) setLast(id);
  }, [id]);
  return id ?? last;
}

export function Shell({ children }: { children: ReactNode }) {
  const [collapsed, toggleFleet] = useFleetCollapsed();
  const [overlay, setOverlay] = useState<Overlay>(null);
  const session = useLastSession();
  useKeys(toggleFleet, setOverlay);
  const show = (o: Overlay) => () => {
    setOverlay(o);
  };
  const close = show(null);
  return (
    <SessionsProvider>
      {/* A narrow touch screen with the terminal focused (the soft keyboard up): the shell's chrome
          yields its rows to the terminal, as the session's header does. */}
      <div className="flex h-full max-sm:flex-col max-lg:pointer-coarse:[&:has(.xterm:focus-within)_[data-chrome]]:hidden">
        <Nav
          session={session}
          onFleet={show("fleet")}
          onKeys={show("keys")}
          onMore={show("palette")}
        />
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          <TopBar fleetCollapsed={collapsed} onFleet={toggleFleet} onPalette={show("palette")} />
          <div className="flex min-h-0 flex-1">
            <FleetColumn collapsed={collapsed} onCollapse={toggleFleet} />
            <main className="min-w-0 flex-1">{children}</main>
          </div>
        </div>
      </div>
      <FleetSheet open={overlay === "fleet"} onClose={close} />
      <Palette open={overlay === "palette"} onClose={close} />
      <CheatSheet open={overlay === "keys"} onClose={close} />
      <Toaster />
    </SessionsProvider>
  );
}
