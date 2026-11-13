// The command palette (jump to a view or a session) and the cheat sheet of the keys there are.
import { Search, SquareTerminal, PanelsTopLeft } from "lucide-react";
import { useState } from "react";
import { lanePath, navigate, sessionPath, VIEWS } from "@/app/route";
import { useSample } from "@/preview/engine";
import { Dialog } from "@/components/ui/dialog";
import { useSessions } from "@/features/sessions/store";

const MOD = /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘" : "Ctrl";

/** Every key the Workbench has, as the cheat sheet lists it. */
export const KEYS = [
  [`${MOD} K`, "Command palette: jump to a view or a session"],
  ["?", "Keyboard shortcuts"],
  [`${MOD} B`, "Collapse or expand the fleet"],
  ["Esc", "Close the palette or this sheet"],
] as const;

interface Target {
  label: string;
  detail: string;
  href: string;
  session: boolean;
}

export function Palette({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Dialog open={open} onClose={onClose} label="Command palette" className="mt-[12vh] mb-auto">
      <Targets onClose={onClose} />
    </Dialog>
  );
}

/** Mounted with the dialog, so each opening starts with an empty query. */
function Targets({ onClose }: { onClose: () => void }) {
  const { state } = useSessions();
  const { lanes } = useSample();
  const [query, setQuery] = useState("");
  const all: Target[] = [
    ...VIEWS.map((v) => ({ label: v.label, detail: "View", href: v.path, session: false })),
    ...lanes.map((l) => ({
      label: `${l.key} ${l.title}`,
      detail: "Sample lane",
      href: lanePath(l.key),
      session: true,
    })),
    ...(state.status === "ready" ? state.sessions : []).map((s) => ({
      label: `${s.ticketKey} ${s.title}`,
      detail: s.repo || "Session",
      href: sessionPath(s.id),
      session: true,
    })),
  ];
  const q = query.trim().toLowerCase();
  const shown = all.filter((t) => t.label.toLowerCase().includes(q));
  const go = (t: Target): void => {
    onClose();
    navigate(t.href);
  };
  return (
    <div className="flex max-h-[70vh] flex-col">
      <label className="flex items-center gap-2 border-b border-line px-3">
        <Search className="size-4 text-faint" aria-hidden />
        <input
          autoFocus
          type="search"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
          }}
          onKeyDown={(e) => {
            const first = shown[0];
            if (e.key === "Enter" && first) go(first);
          }}
          placeholder="Jump to a session or a view…"
          aria-label="Jump to a session or a view"
          className="h-12 min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-faint"
        />
      </label>
      <ul className="min-h-0 flex-1 overflow-y-auto p-1.5">
        {shown.map((t) => (
          <Row key={t.href} target={t} go={go} />
        ))}
        {!shown.length && <li className="px-2.5 py-3 text-[12.5px] text-dim">Nothing matches.</li>}
      </ul>
    </div>
  );
}

function Row({ target: t, go }: { target: Target; go: (t: Target) => void }) {
  return (
    <li>
      <button
        type="button"
        onClick={() => {
          go(t);
        }}
        className="flex min-h-11 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-[13px] hover:bg-hover focus-visible:bg-hover"
      >
        {t.session ? (
          <SquareTerminal className="size-4 shrink-0 text-dim" aria-hidden />
        ) : (
          <PanelsTopLeft className="size-4 shrink-0 text-dim" aria-hidden />
        )}
        <span className="min-w-0 flex-1 truncate">{t.label}</span>
        <span className="shrink-0 font-mono text-[11px] text-faint">{t.detail}</span>
      </button>
    </li>
  );
}

export function CheatSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Dialog
      open={open}
      onClose={onClose}
      label="Keyboard shortcuts"
      className="w-[min(480px,calc(100vw-32px))]"
    >
      <div className="p-5">
        <h2 className="text-[14px] font-semibold">Keyboard shortcuts</h2>
        <dl className="mt-4 flex flex-col gap-2 text-[12.5px]">
          {KEYS.map(([keys, what]) => (
            <div key={keys} className="flex items-center gap-3">
              <dt className="order-last ml-auto">
                <kbd className="rounded-md border border-line bg-card px-1.5 py-0.5 font-mono text-[11px] text-dim">
                  {keys}
                </kbd>
              </dt>
              <dd className="text-t2">{what}</dd>
            </div>
          ))}
        </dl>
      </div>
    </Dialog>
  );
}
