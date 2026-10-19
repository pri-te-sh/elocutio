# Progress tracker

The [build plan](PLAN.md) is the source of truth for scope. This tracker is the
source of truth for **execution**: which work items are done, what evidence
proves it, and how far each milestone has moved.

It is designed to stay honest without manual bookkeeping. Task status is
derived from repository evidence — files, globs, text markers, git remotes, and
commit subjects — and a live dashboard recomputes that state on every request.
You can still override any status by hand when a human judgement call is
required.

```text
progress/tracker.json    committed ledger: milestones, work items, evidence rules
progress/template.html   self-contained dashboard (no external assets or CDNs)
scripts/progress.py      evidence engine, static build, live server, CLI
scripts/test_progress.py unit and HTTP tests
progress/out/            generated snapshots and screenshots (gitignored)
```

## Quick start

```bash
just progress          # build a static snapshot in progress/out/
just progress-serve    # live dashboard at http://127.0.0.1:8787
just progress-check    # validate the ledger and run the tests
```

Without `just`:

```bash
python3 scripts/progress.py build
python3 scripts/progress.py serve --open
python3 scripts/progress.py summary
python3 scripts/progress.py validate
```

The static snapshot is a single HTML file with the payload inlined. It opens
with `file://`, shows exactly what the repository looked like when it was
generated, and disables editing. The live server is the useful working mode: it
re-reads the repository on every request and the page polls it every five
seconds.

## What the dashboard shows

- **Scope progress** across planning, M0–M5, weighted by estimated effort so a
  large task does not count the same as a half-day one.
- **Plan vs. actual**: the percentage of scope that *should* be complete by
  today (from the plan's week windows) next to the actual number, with a
  variance chip.
- **Milestone rail**: one segment per phase with actual fill, planned marker,
  task counts, and status.
- **Work items**: every task with area, priority, estimate, evidence coverage,
  dependency waiting list, notes, and an inline status editor (live mode).
- **Acceptance criteria** per milestone, plus a blocker panel, the build-plan
  risks, recent commits with a 14-day activity chart, and a status-change
  history.
- **Filters** by phase, status, area, and free-text search; the shortcut `/`
  focuses search and `Esc` clears it.

The page respects light/dark preferences, avoids external requests, works
offline once loaded, and includes a print stylesheet.

## How status is derived

Every work item has an optional `evidence` list. Each rule is evaluated against
the repository; a status is derived from the result:

| Evidence result | Effective status |
| --- | --- |
| Every rule passes | `done` |
| Some rules pass | `in_progress` |
| No rule passes (or no rules) | `planned` |

Manual overrides always win over evidence. A manual status that lags behind
evidence (for example, `planned` while every rule passes) is flagged in the UI
as **evidence → done**, so stale bookkeeping is visible rather than silent.

Supported rules:

```json
{ "type": "path",        "path": "UPSTREAM.md" }
{ "type": "glob",        "pattern": "apps/macos/src-tauri/src/**/*.rs" }
{ "type": "text",        "path": "docs/ARCHITECTURE.md", "contains": "## 10. IPC" }
{ "type": "git-remote",  "name": "upstream" }
{ "type": "git-commit",  "pattern": "^fork:" }
```

Paths and glob patterns must stay relative to the repository; absolute paths
and `..` segments are rejected by `validate`. `git-commit` checks the subjects
of the most recent 400 commits.

Evidence is a **hint that code or documentation landed**, not proof that a gate
passed. Gates that need human verification use a file as their evidence. M0,
for example, is only done when the five-app overlay results are recorded in
`docs/M0-VERIFICATION.md`; the forked source alone is not enough.

Dependencies are advisory: `dependsOn` lists task ids, and unfinished
dependencies appear as a **waiting** badge with a tooltip. They do not block
edits or auto-change a status.

## Changing status

Three equivalent paths:

1. **Dashboard** (live mode) — use the status picker on any work item. The
   choice is written to `progress/tracker.json` immediately and recorded in the
   history.
2. **CLI** — pass `auto` to remove an override and fall back to evidence:

   ```bash
   python3 scripts/progress.py status m0-build done --note "overlay verified in 5 apps"
   python3 scripts/progress.py status m0-build auto
   ```

3. **Edit `progress/tracker.json`** — add `"status": "done"` (or
   `in_progress`, `blocked`, `planned`) to a task. Run
   `python3 scripts/progress.py validate` afterwards.

Every manual change appends `{ at, task, from, to, note }` to `history`, capped
at 500 entries. The API validates task ids and status values; it never accepts
arbitrary paths.

## The ledger

```jsonc
{
  "schemaVersion": 1,
  "updatedAt": "2026-10-19T16:45:00Z",
  "project": { "name": "Elocutio", "startDate": "2026-10-19", "planWeeks": [14, 19] },
  "areas": { "build": "macOS build" },
  "priorities": { "P0": "Gate-critical" },
  "milestones": [
    {
      "id": "M0",
      "name": "Fork, rebrand, spike",
      "outcome": "Rebranded app builds and runs …",
      "estimateDays": [5, 7],
      "weeks": [1, 1],
      "tasks": [
        {
          "id": "m0-build",
          "title": "Build and run on macOS …",
          "area": "build",
          "priority": "P0",
          "estimateDays": 1.5,
          "status": "in_progress",
          "notes": "Record gate results in docs/M0-VERIFICATION.md.",
          "dependsOn": ["m0-fork"],
          "evidence": [{ "type": "glob", "pattern": "apps/macos/src-tauri/src/**/*.rs" }]
        }
      ],
      "acceptance": [{ "text": "Overlay works in five apps", "accepted": false }]
    }
  ],
  "blockers": [],
  "risks": [],
  "history": []
}
```

Rules enforced by `validate`:

- unique milestone and task ids; task ids match `[a-z0-9][a-z0-9-]*`;
- every `area` and `priority` is declared at the root;
- estimates are non-negative, `weeks` satisfy `0 <= start <= end`;
- `dependsOn` references existing tasks and a task cannot depend on itself;
- acceptance items have text and a boolean `accepted`;
- blockers and history reference existing tasks.

**Weighting.** Overall scope progress is the sum of `milestone weight ×
milestone progress`, where the weight is the midpoint of the milestone's
estimate range and milestone progress is the done estimate divided by total
task estimate. Planning (week 0) carries weight too, so the baseline is not
zero. Tasks without an estimate count as one day.

**Schedule.** Planned windows are derived from `project.startDate` and each
milestone's `weeks` pair. "Planned by today" uses the fraction of each window
that has elapsed. Remaining effort and the projected finish subtract completed
estimate days per phase and add the remainder to today, at one engineer.

## Live API

The server binds to `127.0.0.1` by default and is a local development tool; it
has no authentication and should not be exposed to a network. It refuses
malformed requests and caps the body at 64 KiB.

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/` | dashboard HTML with the payload inlined |
| `GET` | `/api/progress` | complete computed payload (fresh on every request) |
| `GET` | `/api/health` | `{ ok, updatedAt, generatedAt }` |
| `PATCH` | `/api/tasks/<id>` | `{ "status": "done", "note": "optional" }`; returns the new payload (404 for an unknown task) |

Polling pauses while the tab is hidden and refreshes immediately when it
becomes visible. If the server stops, the header switches to “Reconnecting…”
and retries automatically. `Ctrl+C` stops the server.

## CI

`just check` includes the ledger validation, and the Docs CI job runs:

```bash
python3 -m unittest discover -s scripts -p "test_*.py" -v
python3 scripts/progress.py validate
```

The tests use temporary repositories, so they never touch the committed
tracker. `progress/out/` is gitignored; the static snapshot is for local use or
for a future Pages job.

## Troubleshooting

- **Port in use** — the server tries the requested port and the next 19, then
  prints the URL it actually bound.
- **Evidence missing on a fresh clone** — some evidence is environment-specific
  (`.upstream/` is gitignored, `git-remote` is local config). Use a manual
  override; the dashboard will show the evidence result rather than hiding it.
- **Status will not change** — a manual override always wins. Choose **Auto** or
  run `python3 scripts/progress.py status <id> auto` to return to evidence.
- **Tracker invalid** — `python3 scripts/progress.py validate` prints every
  schema and consistency error with the field name.
- **Static page does not update** — by design. Rebuild, or use
  `just progress-serve` for live data.
