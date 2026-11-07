import { runtimeContract, SESSION } from "./contract.ts";
import { FakeRuntime } from "./fake.ts";

runtimeContract("fake", () => {
  const sessions = new Set<string>();
  return (onEvent) => Promise.resolve(new FakeRuntime({ onEvent, sessionId: SESSION, sessions }));
});
