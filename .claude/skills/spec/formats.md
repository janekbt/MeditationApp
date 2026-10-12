# /spec file formats

The files follow OpenSpec's formats (github.com/Fission-AI/OpenSpec), so
its CLI could be adopted later without moving anything. We don't use
the CLI; write the files by hand.

```
openspec/
├── specs/<area>/spec.md          living specs: what the app does now
└── changes/
    ├── <kebab-name>/             one change in progress
    │   ├── .openspec.yaml
    │   ├── proposal.md
    │   ├── tasks.md
    │   ├── design.md             only for schema, sync, wire format, new dependency
    │   └── specs/<area>/spec.md  deltas
    └── archive/YYYY-MM-DD-<kebab-name>/
```

The archive is history. It never describes the app as it is now; the
living specs and the code do.

## Areas

An area gets its `spec.md` the first time a change touches it; never
back-fill a whole area. Expected areas: `sessions`, `timer`, `bells`,
`vibration`, `box-breath`, `guided`, `labels`, `presets`, `log`, `stats`,
`sync`, `backup-import`, `preferences`, `translations`. Add a new one only
when no existing area fits.

## Living spec

```markdown
# Labels

## Purpose
Labels group sessions by kind of practice. They are named by the user,
synced between devices, and shown on sessions in the Log and in Stats.

## Requirements

### Requirement: Same names settle the same way on every device
When two devices create a label with the same name, every device SHALL
keep the name on the label with the smaller id and suffix the other.

#### Scenario: Both devices create "Walking" before their first sync
- **WHEN** two devices each create "Walking" and then sync
- **THEN** both show the same label as "Walking" and the same one with "(conflict-...)"
- **TEST** `meditate-core/src/db/labels.rs::every_device_suffixes_the_same_row_of_a_same_name_pair`
```

Rules (OpenSpec's validator enforces the first four):
- `## Purpose` (at least 50 characters) and `## Requirements` are required.
- `### Requirement: <name>`; the line right after it holds the SHALL or
  MUST statement. One statement, one SHALL. Body under 500 characters.
- Every requirement has at least one `#### Scenario:` (exactly four `#`).
- Scenario lines: `- **WHEN**`, `- **THEN**`, `- **AND**`.
- Every scenario ends with exactly one proof line:
  - `- **TEST** \`<file>::<test fn name>\`` for an automated test;
  - `- **MANUAL** <what a person does and sees>` only when no test can
    run it (sound, vibration, layout on a phone). Say why in the line.
- Describe behaviour a user or another device can see. No function,
  table or type names outside the proof line. If the code can change
  without changing what anyone sees, it does not belong in the spec.
- Name the edge case in the scenario title ("...while offline",
  "...after the label was deleted").

`build-aux/check-specs.sh` checks that every `**TEST**` line in
`openspec/specs/` and in open changes points at a test function that
exists in that file.

## Change folder

`.openspec.yaml`:
```yaml
schema: spec-driven
created: 2026-10-12
```
Add `skip_specs: true` only for a change with no behaviour (a pure
refactor); never invent a requirement to fill a delta.

### proposal.md

```markdown
# <Change name>

## Why
<The problem, from the user's side, in 2 to 5 sentences. OPEN-ISSUES
numbers if any.>

## What Changes
- <one line per behaviour that changes>
- Left out: <what was dropped in the scope pushback, and why>

## Decisions
- <each decision settled in step 1, with the human's answer>
- <a minor choice you made yourself, marked "(my call)">

## Capabilities
### New Capabilities
- `<area>`: <one line>
### Modified Capabilities
- `<area>`: <one line>

## Impact
- Core: <modules>
- GTK: <files or "none">
- Android: <files or "none">
- Translations: <new or changed strings, both apps, or "none">
- Data: <schema / sync events / migration, or "none">
- Android build environment: <"untouched", or what and why>
- Docs: <which ones change>
```

### Deltas: `specs/<area>/spec.md` inside the change

```markdown
## ADDED Requirements
### Requirement: <new name>
...full requirement with scenarios...

## MODIFIED Requirements
### Requirement: <exact existing name>
...the WHOLE requirement as it will read, every scenario, changed or not...

## REMOVED Requirements
### Requirement: <exact existing name>
**Reason**: <why>
**Migration**: <what replaces it, or "none">

## RENAMED Requirements
- FROM: `### Requirement: <old name>`
- TO: `### Requirement: <new name>`
```

- MODIFIED copies the entire requirement. A scenario missing from it
  would be lost at archive time; adding a new concern without changing
  an old one is ADDED, not MODIFIED.
- A delta for an area that has no spec yet starts with `## Purpose`.
- Use only the sections you need.

### tasks.md

```markdown
# Tasks: <change name>

Base: <commit hash from step 1>; suites on it: <green, or what failed>

## Loop
- [ ] 1 Understand
- [ ] 2 Propose (approved: <date>)
- [ ] 3 Build
- [ ] 4 Review
- [ ] 5 Specs, docs, list
- [ ] 6 Verify and hand over
- [ ] 7 Ship

## Review Focus
- <input the scenarios are silent on> -> <scenario name, or why not>

## 1. <group, usually one per scenario or issue>
- [ ] 1.1 <task>. Files: <path>. Proof: <test name, Expected: FAIL then PASS>

## Notes
- <test that passed at once, rulings marked "my call", rejected review
  findings with the reason, anything the next session must know>
```

Only `[x]` counts as done. Every task says how to check it is done.
Testing and docs are not a final group; they belong to the task they
prove or describe.

### design.md (only when needed)

`## Context`, `## Goals / Non-Goals`, `## Decisions` (each with the
alternative you rejected), `## Risks / Trade-offs`, `## Migration Plan`
(for schema or sync changes: how a database from the last release is
upgraded, and the test that proves it), `## Open Questions`.

## Archive

At ship time, merge each delta into `openspec/specs/<area>/spec.md` in
this order: RENAMED, REMOVED, MODIFIED (replace the whole requirement),
ADDED (append). A new area gets the delta's Purpose. Then move the
change folder to `openspec/changes/archive/YYYY-MM-DD-<kebab-name>/` and
run `build-aux/check-specs.sh`.
