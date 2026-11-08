import { DesiredState, Phase } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

interface Look {
  label: string;
  hint: string;
  tone: string;
}

const STATES: Record<DesiredState, Look> = {
  [DesiredState.HOT]: {
    label: "Hot",
    hint: "wanted running",
    tone: "border-rl/40 bg-rl-soft text-rl",
  },
  [DesiredState.WARM]: {
    label: "Warm",
    hint: "wanted hibernated",
    tone: "border-warn/45 bg-warn-soft text-warn",
  },
  [DesiredState.COLD]: {
    label: "Cold",
    hint: "wanted torn down",
    tone: "border-line-2 bg-surface-2 text-dim",
  },
  [DesiredState.UNSPECIFIED]: {
    label: "Unknown",
    hint: "no state yet",
    tone: "border-line-2 text-faint",
  },
};

const QUIET = "border-line-2 text-dim";
const PHASES: Record<Phase, Look> = {
  [Phase.UNSPECIFIED]: {
    label: "Not reported",
    hint: "no runner report yet",
    tone: "border-line-2 text-faint",
  },
  [Phase.QUEUED]: { label: "Queued", hint: "waiting for capacity", tone: QUIET },
  [Phase.STARTING]: { label: "Starting", hint: "pod starting", tone: QUIET },
  [Phase.RUNNING]: {
    label: "Running",
    hint: "pod running",
    tone: "border-ok/40 bg-ok-soft text-ok",
  },
  [Phase.HIBERNATING]: {
    label: "Hibernating",
    hint: "pod stopping, work kept",
    tone: "border-warn/45 bg-warn-soft text-warn",
  },
  [Phase.HIBERNATED]: {
    label: "Hibernated",
    hint: "no pod, work kept",
    tone: "border-warn/45 bg-warn-soft text-warn",
  },
  [Phase.WAKING]: { label: "Waking", hint: "pod coming back", tone: QUIET },
  [Phase.TERMINATING]: { label: "Terminating", hint: "pod being removed", tone: QUIET },
  [Phase.GONE]: { label: "Gone", hint: "pod removed", tone: "border-line-2 bg-surface-2 text-dim" },
  [Phase.FAILED]: {
    label: "Failed",
    hint: "the runner reported a failure",
    tone: "border-bad/40 bg-bad-soft text-bad",
  },
};

export const stateLabel = (s: DesiredState): string => STATES[s].label;
export const phaseLabel = (p: Phase): string => PHASES[p].label;

function Chip({ look }: { look: Look }) {
  return (
    <Badge className={cn("gap-1.5 font-mono text-[11px]", look.tone)} title={look.hint}>
      {look.label}
      <span className="sr-only"> ({look.hint})</span>
    </Badge>
  );
}

/** A session's desired state (ADR 0012): hot, warm or cold. */
export function StateBadge({ state }: { state: DesiredState }) {
  return <Chip look={STATES[state]} />;
}

/** Where a session's pod is, as its runner last reported it. */
export function PhaseBadge({ phase }: { phase: Phase }) {
  return <Chip look={PHASES[phase]} />;
}
