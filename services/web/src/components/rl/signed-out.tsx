import { LogIn } from "lucide-react";
import { signInAgain, signedOutHere } from "@/api/client";
import { Button } from "@/components/ui/button";
import { Notice } from "./notice";

/** A view the api refused for want of a login, when the redirect to sign in didn't happen: the
 * member signed out in this tab, or the browser came back from sign-in still signed out
 * (`signIn`'s loop guard). */
export function SignedOut() {
  return (
    <Notice
      icon={LogIn}
      title="You're signed out"
      action={
        <Button
          onClick={() => {
            signInAgain();
          }}
        >
          <LogIn aria-hidden /> Sign in
        </Button>
      }
    >
      {signedOutHere()
        ? "Sign in to see your sessions."
        : "Sign in to see your sessions. If you just did, your sign-in didn't stick: try once more."}
    </Notice>
  );
}
