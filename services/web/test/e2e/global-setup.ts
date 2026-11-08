import { startStub } from "./stub";

export default async function globalSetup(): Promise<() => Promise<void>> {
  const server = await startStub();
  return () =>
    new Promise((resolve) => {
      server.close(() => {
        resolve();
      });
    });
}
