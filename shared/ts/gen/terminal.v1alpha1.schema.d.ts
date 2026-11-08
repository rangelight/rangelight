// Generated from api/jsonschema/ by scripts/check.sh gen. Do not edit.

/**
 * One WebSocket text frame between the Workbench and the api, on GET /v1alpha1/sessions/{session}/terminal with the subprotocol terminal.v1alpha1.rangelight.dev. The api bridges the frames to the NATS subjects pty.<session>.in and pty.<session>.out; the browser never reaches NATS or a pod (ADR 0040 §1). Bytes travel as base64, so any byte sequence, split anywhere, survives a text frame.
 */
export type RangelightTerminalMessageV1Alpha1 = TerminalInput | TerminalOutput | TerminalResize;
/**
 * Raw bytes, standard base64 with padding: at most 48 KiB a frame.
 */
export type Bytes = string;

/**
 * Browser to api: what the member typed or pasted, for the PTY's stdin.
 */
export interface TerminalInput {
  type: "input";
  data: Bytes;
}
/**
 * Api to browser: what the PTY wrote.
 */
export interface TerminalOutput {
  type: "output";
  data: Bytes;
}
/**
 * Browser to api: the terminal's size in character cells changed.
 */
export interface TerminalResize {
  type: "resize";
  cols: number;
  rows: number;
}
