// Repos: the repos your real sessions work in (the api's), then the sample team's, with what the
// preview profile would set up.
import { useSessions } from "@/features/sessions/store";

export interface Repo {
  name: string;
  sample: boolean;
  /** Real: how many of your sessions work in it. */
  sessions?: number;
  branch?: string;
  profile?: string;
}

const SAMPLE: Repo[] = [
  {
    name: "storefront",
    sample: true,
    branch: "main",
    profile: "6 services · 2 seeds · hot-reload storefront, ledger",
  },
  { name: "sdk", sample: true, branch: "main", profile: "type checks only" },
];

export function useRepos() {
  const { state } = useSessions();
  const counts = new Map<string, number>();
  if (state.status === "ready")
    for (const s of state.sessions) if (s.repo) counts.set(s.repo, (counts.get(s.repo) ?? 0) + 1);
  const real: Repo[] = [...counts].map(([name, sessions]) => ({ name, sample: false, sessions }));
  return { real, sample: SAMPLE };
}
