import { LogIn } from "lucide-react";
import { SIGN_IN } from "@/api/client";
import { Button } from "@/components/ui/button";
import { Notice } from "./notice";

/** A view the api refused for want of a login, when the redirect to sign in didn't happen: the
 * browser came back from it still signed out (`signIn`'s loop guard). */
export function SignedOut() {
  return (
    <Notice
      icon={LogIn}
      title="You're signed out"
      action={
        <Button
          onClick={() => {
            location.assign(SIGN_IN);
          }}
        >
          <LogIn aria-hidden /> Sign in
        </Button>
      }
    >
      Sign in to see your sessions. If you just did, your sign-in didn&apos;t stick: try once more.
    </Notice>
  );
}
