// The lane agent: clone from the mirror, start the dev command once, run Claude Code on the
// ticket, push checkpoints and the work branch back (ADR 0045, ADR 0046).
import { brief, readTicket, resumeBrief } from "./brief/brief.ts";
import { Checkpoints } from "./checkpoint/checkpoint.ts";
import { childEnv, loadConfig } from "./config.ts";
import { CliRuntime } from "./runtime/cli.ts";
import { laneHandler } from "./runtime/lane.ts";
import { connectLane } from "./terminal/terminal.ts";
import { devCommands, startDevCommands } from "./workspace/dev.ts";
import { createGit } from "./workspace/git.ts";
import { prepareWorkspace } from "./workspace/workspace.ts";

// SIGTERM is the pod closing. Before Claude Code runs there is nothing to save: exit at once
// (node may be PID 1, where an unhandled SIGTERM does nothing). Once it runs, stopping it pushes
// the last checkpoint and then exits.
let onSigterm: () => void = () => process.exit(143);
process.on("SIGTERM", () => {
  onSigterm();
});

async function main(): Promise<void> {
  const config = loadConfig();
  const env = childEnv();
  const git = createGit(env);
  const { terminal, emit, attach } = connectLane(config);
  // Both files are validated before anything is cloned or started.
  const ticket = readTicket(config.ticketFile);
  const dev = devCommands(config.profileFile, config.underChange, config.workdir);

  const workspace = await prepareWorkspace({ ...config, git });
  startDevCommands(dev, env, emit);
  const checkpoints = new Checkpoints({
    sessionId: config.sessionId,
    workdir: config.workdir,
    last: workspace.lastCheckpoint,
    git,
  });
  const runtime = new CliRuntime({
    bin: "claude",
    cwd: config.workdir,
    sessionId: config.sessionId,
    claudeConfigDir: config.claudeConfigDir,
    env,
    onOutput: (chunk) => {
      terminal.output(chunk);
    },
    onEvent: laneHandler({
      emit,
      checkpoint: () => checkpoints.save(),
      exit: (code) => process.exit(code),
    }),
  });
  attach(runtime);
  onSigterm = () => {
    runtime.interrupt();
    void runtime.stop();
  };
  // Messages reach runtime.inject() from the NATS inbox (RAN-35, RAN-36).
  if (workspace.resumed) await runtime.resume(config.sessionId, resumeBrief(ticket));
  else await runtime.start(brief(ticket));
}

main().catch((error: unknown) => {
  process.stderr.write(`agent: ${error instanceof Error ? error.message : String(error)}\n`);
  process.exit(1);
});
