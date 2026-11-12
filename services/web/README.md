# web

The Workbench: every session as a row, and each session's live event feed and terminal
(ADR 0037, ADR 0049 §5.1). A static React app; the api serves its data on the same origin.

- **Data** comes only from the api, through `src/api/client.ts`: `SessionService` over connect-es
  (types generated from `api/proto` into `shared/ts/gen/`), and the terminal WebSocket of
  `api/jsonschema/terminal.v1alpha1.schema.json`. The browser never reaches NATS or a pod.
  The Sessions page follows one `WatchSessions` stream; a Session page follows `WatchSession` and
  reads the session once that stream is open (`src/api/live.ts`).
- **Look**: tokens only from `src/styles/tokens.css`, primitives in `src/components/ui`, product
  components in `src/components/rl`, one folder per feature in `src/features/`.
- **Phones** are a first-class target: below 1024 px the session view shows its terminal and feed
  as tabs, the app follows the visual viewport so the terminal refits above the soft keyboard, and
  touch or narrow screens get a key bar (Esc, Tab, Shift+Tab, Ctrl+C, arrows, /, Enter) and 44 px
  targets (the `touch` variant in `tokens.css`). `test/e2e/phone.spec.ts` runs at 360x640 and 390x844.

```sh
pnpm --filter web dev     # http://127.0.0.1:4173, proxying the api paths to :4180
pnpm --filter web test    # vitest, then Playwright against the stub api (test/e2e/stub.ts)
pnpm --filter web exec playwright test --update-snapshots   # after a deliberate visual change
```

`test/screenshots/` holds each view in light and dark: the review artifact for visual changes.
The image (`build/package/docker/web/`) serves the build with unprivileged nginx on port 8080.
