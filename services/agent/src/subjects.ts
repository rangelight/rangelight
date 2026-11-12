// The agent's NATS subject names, the one place they are spelt (api/README.md; ADR 0021, ADR 0051:
// every subject starts with `rl.`). The org and session come from validated config.

/** The lane terminal's input: `input` and `resize` messages for this session's PTY. */
export const ptyIn = (org: string, session: string): string => `rl.${org}.pty.${session}.in`;

/** The lane terminal's output: `output` messages from this session's PTY. */
export const ptyOut = (org: string, session: string): string => `rl.${org}.pty.${session}.out`;
