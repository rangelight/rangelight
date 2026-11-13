import { expect, test } from "vitest";
import { type Action, reduce } from "./engine";
import { CHAPTERS } from "./story";
import { initial, LANES } from "./world";

test("RAN-85 the story has the mock-up's chapters, in order", () => {
  expect(CHAPTERS.map((c) => c.title)).toEqual([
    "Morning",
    "Start from Up next",
    "Cross-repo build",
    "The loop closes itself",
    "Bounded autonomy",
    "Budget wall",
    "Fork",
    "Proof",
  ]);
});

const start = initial();
const sampleLanes = new Set<string>(Object.values(LANES).map((l) => l.key));
const sampleIds = new Set([...start.decisions.map((d) => d.id), ...start.reviews.map((r) => r.id)]);

/** An action names only the sample world's lanes, decisions and pull requests. */
function sampleOnly(act: Action): void {
  if ("lane" in act) expect(sampleLanes).toContain(act.lane);
  if ("key" in act && typeof act.key === "string") expect(sampleLanes).toContain(act.key);
  if ("id" in act) expect(sampleIds).toContain(act.id);
  if ("review" in act) expect(sampleIds).toContain(act.review);
}

test("RAN-85 every chapter after the first drives the sample world, and only its sample lanes", () => {
  let w = start;
  for (const chapter of CHAPTERS) {
    const before = w;
    const acts = chapter.steps.flatMap((s) => (s.act ? [s.act] : []));
    acts.forEach(sampleOnly);
    w = acts.reduce(reduce, w);
    if (chapter.id !== "morning") expect(w, chapter.title).not.toBe(before);
  }
});
