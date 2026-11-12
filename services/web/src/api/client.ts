// The one module that talks to the api, and the one place a signed-out visitor is sent to sign in.
// The api and the Workbench share an origin (ADR 0040 §4), so nothing here names a host.
import { Code, ConnectError, createClient, type Interceptor } from "@connectrpc/connect";
import { createConnectTransport } from "@connectrpc/connect-web";
import { AuthService } from "@gen/proto/rangelight/auth/v1alpha1/auth_pb.ts";
import { SessionService } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";

/** Where a signed-out visitor signs in; the api sends them back to "/" (it takes no return path). */
export const SIGN_IN = "/auth/login";

/** When we last sent the browser to sign in, so coming back still signed out doesn't loop. */
const SENT_AT = "rl-sign-in-at";
const LOOP_MS = 10_000;

/** Sends the browser to sign in, unless it was sent there in the last 10 seconds: then it came
 * back still signed out, and the view's notice offers "Sign in" instead of a redirect loop. */
export function signIn(now = Date.now()): void {
  try {
    if (now - Number(sessionStorage.getItem(SENT_AT)) < LOOP_MS) return;
    sessionStorage.setItem(SENT_AT, String(now));
  } catch {
    // No storage (a private window, say): no guard, so no redirect either; the notice offers it.
    return;
  }
  location.assign(SIGN_IN);
}

/** Whether a failed call's reason (`describe` in live.ts) says the login is missing or over. */
export const signedOut = (reason: string): boolean =>
  reason.startsWith(`${Code[Code.Unauthenticated]}:`);

// ponytail: only a call that fails as it starts (a 401, as the api answers without a login) is
// caught here; a stream the api ends mid-way for an expired login shows "Reconnect", whose call is.
const redirectSignedOut: Interceptor = (next) => async (req) => {
  try {
    return await next(req);
  } catch (error) {
    if (ConnectError.from(error).code === Code.Unauthenticated) signIn();
    throw error;
  }
};

const transport = createConnectTransport({
  baseUrl: location.origin,
  interceptors: [redirectSignedOut],
});

/** The session API: GetSession, WatchSession and WatchSessions. */
export const sessions = createClient(SessionService, transport);

/** WhoAmI: the signed-in member. */
export const auth = createClient(AuthService, transport);

/** Ends the login (the api's same-origin check sees this page's Origin), then goes to sign in.
 * Only the api's 204 means the login ended: on any other answer, or none, it kept the login and its
 * cookie, so this stays on the page and says false. */
export async function signOut(): Promise<boolean> {
  const answer = await fetch("/auth/logout", { method: "POST" }).catch(() => null);
  if (answer?.status !== 204) return false;
  location.assign(SIGN_IN);
  return true;
}

/** The WebSocket subprotocol of api/jsonschema/terminal.v1alpha1.schema.json. */
export const TERMINAL_PROTOCOL = "terminal.v1alpha1.rangelight.dev";

/** Opens a session's terminal (api/README.md, "Terminal"). The browser can't see why an upgrade
 * was refused, so a socket that closes before it opened asks the api once: a 401 there signs in. */
export function openTerminal(sessionId: string): WebSocket {
  const url = new URL(
    `/v1alpha1/sessions/${encodeURIComponent(sessionId)}/terminal`,
    location.href,
  );
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  const socket = new WebSocket(url, [TERMINAL_PROTOCOL]);
  let opened = false;
  socket.addEventListener("open", () => {
    opened = true;
  });
  socket.addEventListener("close", () => {
    if (!opened) void sessions.getSession({ sessionId }).catch(() => undefined);
  });
  return socket;
}
