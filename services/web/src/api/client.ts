// The one module that talks to the api (RAN-37 adds the sign-in cookie and redirect here).
// The api and the Workbench share an origin (ADR 0040 §4), so nothing here names a host.
import { createClient } from "@connectrpc/connect";
import { createConnectTransport } from "@connectrpc/connect-web";
import { SessionService } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";

const transport = createConnectTransport({ baseUrl: location.origin });

/** The session API: GetSession, WatchSession and WatchSessions. */
export const sessions = createClient(SessionService, transport);

/** The WebSocket subprotocol of api/jsonschema/terminal.v1alpha1.schema.json. */
export const TERMINAL_PROTOCOL = "terminal.v1alpha1.rangelight.dev";

/** Opens a session's terminal (api/README.md, "Terminal"). */
export function openTerminal(sessionId: string): WebSocket {
  const url = new URL(
    `/v1alpha1/sessions/${encodeURIComponent(sessionId)}/terminal`,
    location.href,
  );
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return new WebSocket(url, [TERMINAL_PROTOCOL]);
}
