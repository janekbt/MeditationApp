---
name: spec
description: Use when starting any change to Meditate's code or behaviour - a bug fix, one OPEN-ISSUES item or a batch of them, a feature, or a refactor beyond a single file. Invoked as /spec <issue numbers, or the idea in a few words>.
---

# /spec: one change, from plan to commit

Every change to Meditate goes through this loop. Specs say what the app
does; tests prove it; the change folder is the memory that survives a
context reset, so keep it current as you go, not at the end.

Violating the letter of these rules is violating their spirit.

File formats: [formats.md](formats.md). Review brief:
[reviewer-prompt.md](reviewer-prompt.md). Project rules, commands and
the doc map: `AGENTS.md`.

## Size first

| The change | What it needs |
|---|---|
| Typo, comment, doc-only, translation fix | No change folder. Steps 5 to 7 still apply. |
| Anything that changes what the app does, or touches more than one file | The full loop. |
| A batch of OPEN-ISSUES items | One change folder for the batch; the proposal lists the issue numbers. |

Unsure? Use the full loop.

## The loop

Keep the checklist at the top of `tasks.md` and tick each step when
it is done. A skipped step stays unticked with its reason on the line.

### 1. Understand

- Read `AGENTS.md`, the living spec of every area the change touches
  (`openspec/specs/<area>/spec.md`), and the OPEN-ISSUES entries.
- Re-check each issue against the current code: does it still happen,
  are the line numbers right? An issue that is already fixed gets a test
  that proves it and leaves the list.
- Record the base: `git rev-parse HEAD` goes into `tasks.md`. The working
  tree must be clean; if it is not, ask what the uncommitted work is.
- Unclear intent: ask the human, one question per message. A guess
  they can correct beats an open question; mark it as a guess.

### 2. Propose

Write the change folder (`openspec/changes/<kebab-name>/`, see
formats.md):

- `proposal.md`: why, what changes, every place it must reach.
- Spec deltas: each behaviour as a requirement with WHEN/THEN scenarios,
  each scenario with the test that will prove it.
- `tasks.md`: the checklist, Review Focus, then the tasks.
- `design.md` only for a schema, sync or wire-format change, or a new
  dependency.

Push back on scope before showing it: drop every part the need does not
justify and say what you dropped in one line. A plan is the set of
decisions the implementer cannot make alone; a plan longer than the code
it describes has written the code instead.

Self-review before showing it:
1. Every issue or request maps to at least one scenario.
2. Every scenario names its test, and a reviewer who never saw the code
   could tell whether it passed.
3. Every place it must reach is listed: core, GTK, Android, the nine
   translations of each app, docs, migration, Android build environment.
4. Review Focus names the inputs the scenarios are silent on (a second
   device, a replayed event, an old database, a deleted row, an empty
   list, another language), and each one got a scenario or a reason.
5. No "TBD", no "handle edge cases", no step that says what without how.

For a large change (a new feature, sync or schema work), first give the
change folder to a fresh agent: "You are a senior Rust developer who
must build this without talking to its author. List only the questions
where a wrong guess would matter." Answer what the files already
answer, decide the minor ones yourself (mark them as your call in
`tasks.md`), and bring the rest to the human once.

```
HARD GATE: nothing is built until the human has read the proposal and
said go. Building includes tests, scaffolding and "just trying it".
Approval covers what you showed, nothing else.
```

### 3. Build, test first

```
NO PRODUCTION CODE WITHOUT A FAILING TEST FIRST.
Code written before its test is deleted, not kept as a reference.
The only exception is a throwaway spike the human approved as one.
```

For each scenario, in the order of `tasks.md`:

1. Write the test the scenario names. Expected values are written by
   hand, never computed by the code under test.
2. Run it and watch it fail for the right reason. A compile error is not
   a failure; make it compile with a stub first. A test that passes at
   once means the behaviour already exists: say so in `tasks.md` and check
   the test would fail if the behaviour broke (break it on purpose, run,
   restore).
3. Write the least code that passes it. Both apps, where the behaviour
   is in both; decisions go in core.
4. Run the whole suite, not just the file. A failure you saw and did
   not report is a false report.
5. Tick the task in `tasks.md`.

A test is good when you can name the production change that would make
it fail. No tests that only restate the code (change detectors), none
that test the framework, none that read source text unless the code
cannot run on the host.

When something breaks you don't understand: find the cause before
fixing anything. Reproduce it, read the whole error, check what changed,
add logging at each boundary (core, GTK, Android, sync, database) until
you see where it goes wrong, then state "X is the cause because Y" and
test that one idea. After three fixes that didn't work, stop and talk
to the human: the design is probably wrong, not the fix.

### 4. Review

Start a fresh agent with [reviewer-prompt.md](reviewer-prompt.md), the
change folder path and the base commit. Give it files, never this
conversation, and never tell it what not to flag.

For each finding: check it against the code yourself. Fix what holds,
one at a time, with a test where it is behaviour. A finding you reject
gets one line in `tasks.md` (what, why) and is shown to the human. A
finding that would change behaviour, UI text or data beyond the approved
proposal is a question for the human, not a fix.

### 5. Specs, docs and the list

- Run `build-aux/check-specs.sh`: every test named in a spec exists.
- Update every doc the change made wrong, using the doc map in
  `AGENTS.md`. Grep the docs for the names you changed.
- OPEN-ISSUES.md: remove the fixed items, tick a finished batch.

### 6. Verify and hand over

Claim nothing you have not just seen:

| Claim | Needs | Not enough |
|---|---|---|
| Tests pass | the full suite's output, read to the end | an earlier run, one file |
| Lints clean | clippy as AGENTS.md lists it, zero warnings | "it compiled" |
| Built | the artifact's new timestamp or hash | "Finished in 15s" |
| Fixed | the original symptom, reproduced and now gone | the test passing |
| Works on the device | the human said so | the build succeeding |

Build what the human tests on (APK, GTK build) and give them test steps:
what to do, what they should see, for each scenario that only a person
can check.

```
HARD GATE: no commit before the human tested and approved.
"Tests are green" is not approval.
```

### 7. Ship

1. Merge the deltas into the living specs (formats.md, "Archive"), move
   the change folder to `openspec/changes/archive/YYYY-MM-DD-<name>/`, run
   `build-aux/check-specs.sh` again.
2. Show the commit message; commit and push to `beta` once approved, in
   one commit with the code, the specs and the archived folder.

## Stop and ask before

- a schema or sync wire-format change (it needs a migration, tested
  against a database from the last release);
- touching the Android build environment (the list in `AGENTS.md`);
- adding a dependency;
- deleting files or user data;
- any change to behaviour, UI text or data that the approved proposal
  does not cover;
- a commit or a push.

## What you will tell yourself

| The thought | The truth |
|---|---|
| "Too small for a change folder." | Small changes skip the most steps and break the most rules. Use the size table. |
| "The tests are green, I'll commit and the human tests later." | No commit before the device test. Hard gate. |
| "I'll update the docs and specs at the end." | The end is when they get forgotten. Step 5 is before the hand-over. |
| "The other app can follow later." | Both apps, in the same change, or a written reason the human approved. |
| "The test passed first time, good." | Either it tests nothing or the behaviour existed. Find out which. |
| "It built." | Stat the artifact. |
| "The reviewer found nothing, so it's right." | The review is one check of several; step 6 still applies. |
| "They said go earlier." | Approval covers what you showed. New scope needs a new go. |
| "An `#[allow(...)]` silences it." | Fix the cause. |
| "I followed the spirit of the rule." | The letter is the spirit. |
