// The Services panel: the shrunk product a sample lane runs against.
export interface Service {
  name: string;
  role: string;
  health: "Healthy" | "Progressing";
  source: "hot-reload" | "deploy";
}

const SERVICES: Service[] = [
  { name: "agent", role: "Agent runtime", health: "Healthy", source: "deploy" },
  { name: "storefront", role: "Web shop (Rust)", health: "Healthy", source: "hot-reload" },
  { name: "ledger", role: "Credits ledger (Rust)", health: "Healthy", source: "hot-reload" },
  { name: "postgres", role: "Postgres · ledger schema", health: "Healthy", source: "deploy" },
  { name: "cache", role: "Holds and idempotency keys", health: "Progressing", source: "deploy" },
  { name: "payments-mock", role: "Payments API stand-in", health: "Healthy", source: "deploy" },
];

export function useServices(key: string) {
  return { services: key ? SERVICES : [] };
}
