// Settings: the signed-in member (the api's WhoAmI), then the sample team's settings.
import { useEffect, useState } from "react";
import { auth } from "@/api/client";
import { notify, SAMPLE_ONLY } from "./toast";

const SAMPLE = [
  { label: "Team", value: "acme · storefront team" },
  { label: "Default lane budget", value: "$8 · 90 min" },
  { label: "Escalate after", value: "3 QA bounces on the same journey" },
  { label: "Notifications", value: "Decisions to you, digests at 09:00" },
];

export function useSettings() {
  const [member, setMember] = useState("");
  useEffect(() => {
    auth.whoAmI({}).then(
      (r) => {
        setMember(r.member);
      },
      () => undefined,
    );
  }, []);
  return {
    member,
    sample: SAMPLE,
    save: (): void => {
      notify(SAMPLE_ONLY);
    },
  };
}
