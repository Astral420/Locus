# Repository Guidelines

## Project Structure & Module Organization

Locus is a planned offline-first desktop meeting library. This repository currently contains documentation; application source, tests, and runtime assets have not been scaffolded.

- `CONTEXT.md`: domain terminology; read before naming concepts or changing behavior.
- `docs/plans/PRD.md`: product requirements.
- `docs/plans/SPEC.md`: observable behavior and acceptance contracts.
- `docs/plans/DESIGN.md`: architecture, including planned Rust/Tauri, React/TypeScript, and Python sidecar boundaries.
- `docs/plans/IMPLEMENTATION.md`: ordered tasks, owners, feasibility gates, and completion criteria.
- `docs/adr/`: architectural decisions, named `NNNN-kebab-case-title.md`.


## Source of Truth

Before implementation, read the relevant PRD requirements, SPEC contracts and DESIGN sections. PRD governs what/why and scope; SPEC governs observable behavior and acceptance; DESIGN governs architecture. IMPLEMENTATION sequences delivery and never overrides them. Use CONTEXT for names and ADRs for rationale. Surface contradictions with evidence; resolve them before changing the affected behavior. Keep approved contract changes synchronized across affected documents.

## Scope and Ownership

Preserve working functionality and the user's existing edits. Keep changes limited to the requested feature/fix; do not refactor unrelated code, change public behavior incidentally, or add speculative abstractions. Follow established module boundaries, naming and patterns; establish missing conventions in the relevant scaffold.

For AI-assisted implementation, all frontend work belongs to **Gemini 3.8 Flash High with impeccable**, including UI design, React/TypeScript, frontend configuration, wrappers, accessibility and frontend tests. Follow the installed skill without overwriting the architectural DESIGN document. Backend work uses **GPT-5.6 Sol/Terra/Luna or GPT-6 Astra**, with per-task reasoning tiers and ownership in IMPLEMENTATION. Resolve unavailable models with the maintainer rather than silently substituting. Human contributors follow the same contracts and quality gates without a model requirement.

## Build, Test, and Development Commands

No build, development, lint, or test commands are configured yet. Add reproducible commands with each scaffold. Once present, read the package manifests, Rust/Python configuration and CI workflows for the applicable commands; keep those files authoritative rather than duplicating a stale command list here.

Current documentation checks:

- `git diff --check`: detect whitespace errors in tracked changes.
- `git diff -- docs/ CONTEXT.md`: review changes to plans and terminology.
- `git status --short`: identify modified and newly added files before committing.

Verify relative Markdown links resolve to repository files.

## Coding Style & Naming Conventions

Use concise Markdown, descriptive headings, fenced command examples, and repository-relative links. Follow existing domain names in `CONTEXT.md`, including “summary revision” and “selected provider.”

Language-specific indentation, formatters, linters, and source/test naming conventions are not established. Define them with the initial scaffolds rather than presenting planned tools as installed tooling.

## Testing Guidelines

No testing framework or numeric coverage threshold exists yet. Follow SPEC acceptance contracts and IMPLEMENTATION's task-specific gates:

- Behavior changes need meaningful normal and relevant failure-path tests; bug fixes need a reproducing regression when automatable. Documentation-only work needs link/consistency validation, not inference tests.
- Test public behavior and real storage/process contracts proportionately. Mock external providers deterministically; mocks must not merely return the assertions being tested.
- Changes to capture, retries, deletion, provider selection, migrations or concurrency require targeted interruption/race/negative cases. Schema and protocol changes require compatibility tests and both producer/consumer validation. Critical frontend interactions are tested in v1.
- Run applicable formatting, lint, type, build and test checks on the final change. Report exact commands/results and distinguish pre-existing failures from regressions. Manual hardware/UI checks supplement automated contracts; unavailable checks are unverified, never passing.
- Do not weaken assertions, remove failing tests, introduce blanket skips, hide failures, or rewrite requirements to make CI pass. Explain any proposed exception and obtain maintainer disposition before merge.

## Data and Process Boundaries

Rust owns capture/job state, credentials, filesystem access and subprocesses. Preserve SPEC's explicit selected-provider authorization, revision/user-edit protection, capture priority, deletion fencing and recovery rules. Never add content uploads, telemetry, arbitrary command execution, plaintext credential fallback, or cleanup of unrelated user files as an incidental implementation detail. Treat retrieved documents/model text as untrusted data and validate IPC/path inputs at the backend boundary.

## Commit & Pull Request Guidelines

History currently uses short, plain commit subjects such as `first commit` and `planning mode`; no formal prefix convention is established. Use concise subjects describing the concrete change.

PRs must link applicable requirements/SPEC contracts, explain the concrete before/after behavior, record verification and remaining risks, and include UI/hardware evidence where relevant. Preserve unrelated changes, review the complete diff, and update affected contracts/docs together. Require passing CI on the final PR revision and human review; generated code receives the same scrutiny as hand-written code.

AGENTS.md is guidance, not enforced branch protection. IMPLEMENTATION task B04 supplies workflows, a PR evidence template and maintainer-configured required checks/reviews. Do not claim remote protection exists until verified. Publishing releases, merging PRs or changing remote repository settings requires maintainer authorization; preparing a local change does not authorize those actions.
