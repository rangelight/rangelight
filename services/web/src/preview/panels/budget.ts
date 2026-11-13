// The budget bar: a sample lane's spend against its cap.
import { useLane } from "./lane";

export function useBudget(key: string) {
  const lane = useLane(key);
  const spent = lane?.budget.spent ?? 0;
  const cap = lane?.budget.cap ?? 0;
  return {
    spent,
    cap,
    projected: Math.min(cap, spent * 1.22 + 0.3),
    source: "Team pool · model API",
  };
}
