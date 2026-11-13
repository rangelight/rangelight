# web

The Workbench: every session as a row, and each session's live event feed and terminal
(ADR 0037, ADR 0049 §5.1). A static React app; the api serves its data on the same origin.

- **Data** comes only from the api, through `src/api/client.ts`: `SessionService` over connect-es
  (types generated from `api/proto` into `shared/ts/gen/`), and the terminal WebSocket of
  `api/jsonschema/terminal.v1alpha1.schema.json`. The browser never reaches NATS or a pod.
  The Sessions page follows one `WatchSessions` stream; a Session page follows `WatchSession` and
  reads the session once that stream is open (`src/api/live.ts`).
- **Signed out**: a call the api refuses with 401 (`Unauthenticated`), or a terminal whose upgrade
  is refused, sends the browser to `/auth/login`, which brings it back to `/`. Back within 10
  seconds still signed out, the view shows "You're signed out" with a Sign in button instead. After
  Sign out (a 204) the tab shows that page and stays there: nothing redirects until the member
  presses Sign in, which goes once to `/auth/login?prompt=consent`. That shows Dex's Grant Access
  screen, but Sign out doesn't end the browser's GitHub session: on a shared machine, also sign out
  of GitHub, or the next person can sign in as you with one click.
- **Shell** (`src/features/shell/`, the mock-up's): a rail with a view's icon only once the view
  exists (Sessions, the Fleet wall, Up next, the session last opened); a top bar with the view, the live status of the one
  `WatchSessions` stream every part shares, the member from `AuthService.WhoAmI`, search and Sign out
  (`POST /auth/logout`); the fleet of sessions not torn down, a column that collapses (remembered in
  this browser) or a sheet below 1024 px; a command palette (Ctrl/⌘ K, or Search) and `?` for the
  keys. On phones the rail is a bottom bar, and a session view shows its phase as a stage strip.
- **Fleet wall** (`/wall`) and **Up next** (`/up-next`), from the same stream
  (`src/features/sessions/lanes.ts` says which is which): a tile per live lane (its pod started and
  isn't gone), with its state, phase, last activity and a waiting-for-you mark; and the sessions with
  no lane yet (no runner report, or queued; not torn down), oldest first.
- **Preview facade** (`src/preview/`): the mock-up's views that the api can't feed yet read their
  data only through one typed hook per view (`architect.ts`, `decisions.ts`, `reviews.ts`,
  `coordinator.ts`), each returning data and its actions. A hook mixes in what the api already has
  (sessions, phase, state, questions, PR links) and fills the rest from one live sample world
  (`world.ts`; `engine.ts` runs its actions and moves its lanes on). Every sample item is flagged
  `sample` and shown with `PreviewMark`; an action changes the sample world only and says so.
  A new view adds its own hook file (types in `types.ts`); a real implementation replaces a hook
  without touching its view.
  The session panels (diff, tests, receipt, diagnosis, pod, preview, code, logs, services, history,
  the full stage strip and the budget) each have a hook in `preview/panels/`, keyed by a sample
  lane. A sample lane has its own session view (`/lanes/<key>`), where the panels act. A real
  session's panels only show a marked example of what they will show (`PanelFrame`), with every
  action off: sample data never appears under a real session's identity.
- **Look**: tokens only from `src/styles/tokens.css`, primitives in `src/components/ui`, product
  components in `src/components/rl`, one folder per feature in `src/features/`.
- **Phones** are a first-class target: below 1024 px the session view shows its terminal and feed
  as tabs, the app follows the visual viewport so the terminal refits above the soft keyboard, and
  touch or narrow screens get a key bar (Esc, Tab, Shift+Tab, Ctrl+C, arrows, /, Enter) and 44 px
  targets (the `touch` variant in `tokens.css`). `test/e2e/phone.spec.ts` and `shell.spec.ts` run at
  360x640 and 390x844.

```sh
pnpm --filter web dev     # http://127.0.0.1:4173, proxying the api paths to :4180
pnpm --filter web test    # vitest, then Playwright against the stub api (test/e2e/stub.ts)
pnpm --filter web exec playwright test --update-snapshots   # after a deliberate visual change
```

`test/screenshots/` holds each view in light and dark: the review artifact for visual changes.
The image (`build/package/docker/web/`) serves the build with unprivileged nginx on port 8080.
