# Repository Guidelines

## Project Structure

Locus is a planned offline-first desktop meeting library. Application code and tooling are not scaffolded yet.

- [CONTEXT.md](CONTEXT.md): domain terminology; [docs/adr/](docs/adr/): decision rationale.
- [PRD](docs/plans/PRD.md), [SPEC](docs/plans/SPEC.md), [DESIGN](docs/plans/DESIGN.md): requirements, acceptance contracts and architecture.
- [IMPLEMENTATION](docs/plans/IMPLEMENTATION.md): milestones M1–M10, task IDs, dependencies, acceptance gates and owner assignments.
- [PRODUCT](PRODUCT.md), [root DESIGN](DESIGN.md), [FRONTEND_DESIGN](docs/plans/FRONTEND_DESIGN.md): visual system and frontend design authority.

## Source of Truth

PRD governs what/why; SPEC governs observable behavior and acceptance; DESIGN governs architecture; IMPLEMENTATION sequences delivery (M1–M10 tasks with dependencies and gates) without overriding them. Frontend briefs remain subordinate to architectural DESIGN. Use CONTEXT for names and ADRs for rationale. Resolve contradictions with evidence before changing behavior.

## Scope and Ownership

Preserve existing edits and working functionality. Limit changes to the requested scope; follow module boundaries and conventions without unrelated refactoring or speculative abstractions.

The maintainer coordinates dispatch per IMPLEMENTATION task IDs. Follow task dependencies (`Depends` column) and acceptance gates. The executor finishes only the assigned task, records evidence and stops. No automatic next-task dispatch or model substitution is authorized.

**Model routing:** Backend assignments use the model selected by the maintainer per task (recorded in the Owner column of IMPLEMENTATION.md). All frontend work—React/TypeScript, design, configuration, accessibility and tests—uses **Gemini 3.8 Flash High through Antigravity/AGY with impeccable as handled by maintainer**. Human contributors follow the same contracts/gates without model requirements.

## Build and Development Commands

Use **pnpm** for Tauri/React frontend dependencies. Pin pnpm via `package.json` `packageManager` field; commit `pnpm-lock.yaml`. Rust and Python retain their own tooling.

For documentation changes, run `git diff --check`, validate relative links and inspect the diff plus `git status --short`.

## Coding Style

Use concise Markdown and repository-relative links. Follow CONTEXT's names, including "summary revision" and "selected provider." Establish language-specific formatting, linting and naming conventions with initial scaffolds.

## Testing

- Follow SPEC acceptance contracts and IMPLEMENTATION acceptance gates. Use meaningful normal/failure-path tests, real storage/process boundaries and deterministic provider mocks.
- Capture, retries, deletion, provider authorization, migrations and concurrency need interruption/race/negative cases. Schema/protocol changes require producer/consumer checks; critical frontend interactions require tests.
- Run applicable format, lint, type, build and test checks on the final revision. Report exact results, distinguish pre-existing failures, mark unavailable hardware/UI checks unverified.

## Data and Process Boundaries

Rust owns capture/jobs, credentials, filesystem access and subprocesses. Preserve selected-provider authorization, revision/user-edit protection, capture priority, deletion fencing and recovery. Validate IPC/path inputs at backend boundaries; treat retrieved content and model text as untrusted. Never introduce incidental uploads, telemetry, arbitrary execution, plaintext credential fallback or cleanup of unrelated files.

## Commits and PRs

Concise commit subjects. PRs link IMPLEMENTATION task IDs and SPEC contracts, explain behavior changes, include verification and risks. Merge requires passing CI and human review. Publishing, merging and remote-settings changes require maintainer authorization.
