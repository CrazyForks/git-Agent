---
name: validate-merge-ai-suggestions
description: Validate Git Agent's AI-assisted three-way merge recommendations against an independently derived expert merge baseline. Use for live model self-tests, prompt/context/tool-schema regressions, wrong left/right/manual choices, stale imports or references, missed cross-file dependencies, or evaluating the configured LLM on a real repository that is already in a conflicted merge state.
---

# Validate Merge AI Suggestions

Evaluate the production suggestion path without opening the UI. Derive the correct merge X first, obtain live model advice Y second, then compare them semantically and turn every discovered failure into a regression.

## Preserve the target merge

- Treat the target repository, index, `MERGE_HEAD`, and worktree as read-only.
- Do not run checkout, reset, restore, add, commit, merge, rebase, clean, or conflict-resolution commands.
- Read the three inputs from Git index stages: stage 1 is Base, stage 2 is Left, and stage 3 is Right.
- Keep API keys secret. Print only model name, API format, sanitized endpoint, model ID, target counts, choices, and reasons.
- Use temporary files only when an in-memory focused fixture is required.

## 1. Establish the state

Run read-only checks in the target repository:

```powershell
git status --short
git diff --name-only --diff-filter=U
git rev-parse --abbrev-ref HEAD
git rev-parse -q --verify MERGE_HEAD
```

Stop if there is no intentional merge state or no unmerged file. Never manufacture a merge in a user's real project without explicit permission.

## 2. Derive baseline X before seeing AI output

For every unmerged path, read Base, Left, and Right with `git show :1:<path>`, `:2:<path>`, and `:3:<path>`. Also inspect:

- the current Middle/worktree draft;
- staged auto-merges and unconflicted sibling changes;
- symbol definitions, imports, callers, tests, and configuration;
- branch-specific commits and intent.

Write a target matrix before invoking AI:

| Target | Expected choice/result | Evidence | Confidence |
|---|---|---|---|
| conflict/deletion index | Left, Right, or exact Manual combination | Middle + references + history | high/medium/low |

Apply these rules:

- Treat Middle as authoritative for edits already merged outside the target.
- Do not preserve A merely because one side still contains A. Verify A has a surviving use in the complete Middle result and related code.
- Prefer Manual when the correct result combines both sides or product intent is genuinely ambiguous.
- Check behavior and invariants, not line similarity. Pay special attention to validation order, authorization, fallback paths, feature flags, and imports whose last use disappeared.
- State uncertainty instead of inventing policy values or product decisions.

## 3. Obtain live advice Y through production code

In the Git Agent repository, run the ignored live harness. Set `GIT_AGENT_DATA_DIR` to the running app's data directory so the harness decrypts the existing configured model through the same application path. Never copy or print the secret.

```powershell
$env:GIT_AGENT_LIVE_AI_REPO='D:\path\to\conflicted-repo'
$env:GIT_AGENT_DATA_DIR='D:\workspace\git-Agent\target\debug\data'
$env:GIT_AGENT_LIVE_AI_FILES='src/a.ts;src/b.ts' # optional
$env:GIT_AGENT_LIVE_AI_MODEL='configured name'   # optional
cargo test --lib live_merge_ai_self_test_current_index -- --ignored --nocapture --test-threads=1
```

Also run the fixed stale-reference regression:

```powershell
cargo test --lib live_merge_ai_removed_reference_prefers_deletion -- --ignored --nocapture --test-threads=1
```

These tests call the same context collector, endpoint builder, function-call schema, response parser, and safety guard as the Merge window. They must not modify the target repository. If the development process holds Cargo's build lock, wait for it; do not terminate the user's process.

## 4. Compare X and Y

Compare semantics rather than exact prose. Report:

- coverage: exactly one accepted suggestion per conflict and deletion target;
- decision agreement: Left/Right/Manual versus X;
- proposed Manual content: whether it includes every required edit and correct precedence;
- evidence quality: whether the reason cites Middle, references, tests, or history actually supplied;
- safety: whether it preserves dead imports, bypasses validation, drops a field, or invents intent;
- language/tool validity: bilingual reasons and valid function-call payload.

Classify each mismatch as one of: missing context, misleading prompt, insufficient tool schema, parser loss, deterministic guard gap, model reasoning error, or ambiguous product intent.

## 5. Fix the narrowest responsible layer

- Add missing repository evidence to the context collector when the model could not see it.
- Tighten the prompt when evidence existed but precedence was unclear.
- Extend the function schema only when the current structure cannot express the needed result.
- Add a deterministic safety guard for mechanically provable invariants such as an import with no remaining use.
- Do not encode product judgments as deterministic guards.
- Add ordinary unit tests for deterministic behavior and an ignored live regression for model behavior.

Run targeted tests, `cargo fmt --check`, and `cargo test --lib`. Re-run the live tests only after the deterministic suite passes.

## 6. Deliver evidence

Return a compact X-versus-Y table, the exact disagreements, the responsible layer, changes made, test commands/results, and confirmation that the target merge state is unchanged. Distinguish “the model chose correctly” from “the resulting merged program is proven correct.”
