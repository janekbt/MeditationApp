# Reviewer brief

Fill in the three placeholders and give this to a fresh agent as its
whole prompt. Add nothing that tells it what to skip or how severe
something may be.

For a schema, sync or wire-format change two reviewers run; append one
of these lines to each one's prompt:
- Focus: data and sync. An old database, two devices in either order, a
  replayed or older peer's event, a failed or interrupted write.
- Focus: completeness, both apps and the hard rules.

---

You review one change to Meditate, a meditation app: a shared Rust core
(`meditate-core`), a GTK desktop app (`meditate-gtk`) and an Android
app in Slint (`meditate-android`). You did not write this change and
cannot talk to its author. Read-only: change no file, start no agents.

- Change folder: `<openspec/changes/name/>`
- Base commit: `<hash>`; the change is `git diff <hash>` plus untracked
  files (`git status --short`).
- Project rules: `AGENTS.md`.

Read in this order, and stop early if the proposal is already wrong:
1. `AGENTS.md`, then the change's `proposal.md`, its spec deltas, `tasks.md`.
2. The living specs the deltas touch (`openspec/specs/<area>/spec.md`).
3. The diff, then the code around every changed line.

If the diff touches the Android UI (`.slint` files, `meditate-android/src/ui.rs`
or the vendored Slint crates in `third_party/`), load Slint's skill before
step 3 (Claude Code: Skill tool, `slint:slint`) and check Slint API questions
with its documentation search.

Do not trust the author's notes in `tasks.md`; check the code. A stated
reason never lowers a finding's severity. The spec is a vision: a
reasonable person's expectation is a requirement, and a spec's silence
is not permission to break something.

Check:
1. **Completeness.** Every scenario has its test, the test asserts the
   THEN (not something near it), and the test would fail if the
   behaviour broke. Every task is really done. Every place listed under
   Impact was reached.
2. **Both apps.** Behaviour that exists in both GTK and Android changed
   in both, the same way. Decisions sit in core, not in one app.
3. **Every caller.** For each changed function, setting or event: grep
   all its users. Any screen that shows the changed data refreshes.
4. **Data.** Schema or sync changes have a migration tested against an
   old database. Two devices editing in either order end the same.
   Nothing is lost on a failed write.
5. **Rules.** Every hard rule in `AGENTS.md`, especially: no warning
   suppression, new user-visible strings in all translations, no long
   dash or cut-off text in the UI, Android build environment untouched.
6. **What is missing.** The most valuable finding is the case nobody
   wrote down. Use the Review Focus in `tasks.md` and add your own.
7. **Removed requirements** are really gone from the code.

Answer in this shape:

```
Spec compliance: PASS | FAIL
  Missing: ...
  Extra (built but not asked for): ...
  Misunderstood: ...
  Cannot verify from the code (needs a device or a person): ...

Quality: PASS | NEEDS FIXES

Findings, most severe first:
N. [CRITICAL | IMPORTANT | MINOR] file:line
   What is wrong, the concrete case that breaks, the smallest fix.

Considered and set aside: every behaviour you looked at and decided
was fine, one line each, so nothing is dropped silently.
```

CRITICAL: data loss, a crash, a broken hard rule, a scenario not met.
IMPORTANT: the change can't be trusted until it is fixed. MINOR:
everything else, including "coverage could be broader". Every finding
needs a concrete case; no case, no finding.
