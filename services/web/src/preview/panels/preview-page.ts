// The Preview panel: a sample lane's running product, as a page in its preview namespace.
import { useLane } from "./lane";

export function usePreviewPage(key: string) {
  const lane = useLane(key);
  const k = (lane?.key ?? "").toLowerCase();
  return {
    url: `https://${k}.preview.example/account/usage`,
    rows: [
      { request: "req 7f2a", outcome: "Delivered", credits: "12" },
      { request: "req 7f2b", outcome: "Refunded · 12", credits: "0" },
      { request: "req 7f2c", outcome: "Delivered", credits: "8" },
    ],
  };
}
