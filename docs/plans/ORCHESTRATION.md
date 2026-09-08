# Locus implementation orchestration

**Status:** operating plan; oh my pi installation, model routing and checkpoint enforcement have not been configured or verified by this documentation change.

Use this guide to set up the harness, dispatch implementation tasks, inspect checkpoints, escalate failures and resume interrupted work. [IMPLEMENTATION](./IMPLEMENTATION.md#model-ownership-and-reasoning-tiers) owns model assignments and delivery order. [PRD](./PRD.md), [SPEC](./SPEC.md), [DESIGN](./DESIGN.md) and [AGENTS](../../AGENTS.md) retain their existing authority. This development orchestrator is separate from Locus's Rust runtime pipeline orchestrator.

## Roles and routing

Use **oh my pi** as the development harness. Start the orchestrator on **GPT-6 Astra medium** for task decomposition and integration decisions; use **Astra low** for routine dispatch, evidence inspection and resumption. The orchestrator coordinates work and releases checkpoints; implementation is delegated to the assigned executor.

- **Backend executor:** GPT-5.6 Luna xhigh by default. Dispatch one bounded behavior slice with explicit files, contracts and completion criteria.
- **Complicated backend executor:** GPT-6 Astra low or GPT-5.6 Sol high, following the task owner or the escalation rules below. Use Astra low for unresolved native/platform or architectural questions; Sol high for difficult process, persistence and integration interactions. Return settled implementation slices to Luna xhigh.
- **Backend reviewer:** a separate GPT-5.6 Sol session, medium for bounded changes and high for capture, concurrency, migrations, deletion, provider authorization, protocol/schema changes or complicated executor work.
- **Frontend:** Gemini 3.8 Flash High with impeccable owns all frontend implementation, configuration, wrappers, accessibility and tests. Use a separate Gemini review session for frontend changes. Sol reviews backend and shared wire-contract correctness; frontend fixes stay with Gemini.

Model and effort are separate routing fields. Preserve the requested identity and verify the effective model/effort in each session; a role name or inherited default is insufficient. An unavailable assignment blocks that role until the maintainer resolves it. Model review supplements the required human review.

## Harness setup gate

Before dispatching application work:

1. Pin an oh my pi release or commit and record its version, installation method and configuration location. Consult the [upstream project](https://github.com/can1357/oh-my-pi) for that revision's configuration and agent documentation. Upstream documents subagents and model selection; this guide does not assume a particular configuration schema or automatic checkpoint enforcement.
2. Resolve provider-specific IDs and supported effort values for every role in the ownership table. Record requested and effective settings, with secrets omitted. The official [Luna model page](https://developers.openai.com/api/docs/models/gpt-5.6-luna) documents xhigh support; actual account access and harness routing still require verification.
3. Configure separate orchestrator, executor and review sessions. Load AGENTS and the task packet into each; load TDD for behavior implementation and impeccable for frontend work. Keep review sessions read-only and route fixes to the owner.
4. Run a disposable smoke task to prove role selection, executor handoff, checkpoint return, reviewer isolation and interrupted-session resumption. Verify that an executor stops at a checkpoint until the orchestrator releases it. If the harness cannot enforce this automatically, dispatch only one checkpoint-sized instruction per session and inspect the return before issuing the next.
5. Record setup evidence in `docs/execution/harness.md` when setup is performed. Start application work only after the required roles and checkpoint workflow pass; record unavailable capabilities explicitly.

These are setup completion criteria, not a claim that installing the harness grants merge, release or remote-settings authority.

## Dispatch packet and durable record

The orchestrator creates `docs/execution/<task-id>.md` on first dispatch; this directory is created during execution. Keep one record per task, with separately identified slices. Update it at every checkpoint and before a handoff or interruption. Large redacted logs may live beside it and be linked. Harness chat history alone is insufficient for resumption.

Each record contains:

```text
Task / slice / status / next checkpoint:
Baseline commit / current revision or worktree diff identity:
Orchestrator / executor / reviewer: requested and effective model + effort
Prerequisites and evidence links:
Allowed files/modules and ownership boundaries:
PRD IDs / SPEC sections / DESIGN sections / relevant ADRs:
Public test seam and existing approval reference:
Behavior and independent expected result:
Normal / failure / interruption or race scenarios:
Commands and manual qualification required:
Red: test, command, exit status, expected failure, log, diff identity
Green: command, exit status, log, diff identity
Review: findings, fixes, reruns, final reviewed revision
Unverified checks / risks / escalation history:
Checkpoint decision: continue, rework or blocked; reason and next action
```

Status advances through `scoped → red → green → reviewed → accepted`; use `blocked` with a concrete cause at any stage. A changed contract or implementation reopens affected checks and invalidates stale review evidence. Only the orchestrator marks acceptance after inspecting the evidence.

## Checkpoint protocol

| Checkpoint | Executor return | Orchestrator release criterion |
|------------|-----------------|--------------------------------|
| C0 — scope | Packet, dependency evidence, approved seam, first behavior scenario and test commands | Scope matches contracts; ownership is exclusive; prerequisites pass; test seam approval is recorded; scaffold/spike exceptions are explicit |
| C1 — first red | Small failing test, command/output and explanation of the failure | Test fails for the intended missing behavior, with an independent expected result; unrelated setup/compile failures do not establish behavioral red |
| C2 — slice green | Minimal implementation, red/green log, diff and relevant regression results | Behavior passes at the agreed boundary; normal and applicable adverse cases are covered; no scope drift; next slice is bounded |
| C3 — independent review | Full diff, contract references, evidence and separate reviewer findings | Findings are resolved or receive explicit maintainer disposition; fixes and any review-stage refactoring are rechecked on the resulting revision |
| C4 — acceptance | Final task gate results, applicable checks, manual evidence and remaining limitations | Every task gate is satisfied, final revision matches the review/checks, and downstream consumers have compatible contracts |

Return to the orchestrator at each checkpoint. C1 is required before implementing the first behavior and whenever the seam, contract or risk changes. Within an approved seam, record red before green for each subsequent test and return at C2 after each slice. This keeps the executor working one behavior at a time while giving the orchestrator evidence to inspect before further dispatch.

At C4 for the last task of a phase, also reconcile every phase task and cross-boundary integration result against IMPLEMENTATION. Record a phase decision in the final task's record. Missing hardware evidence leaves the affected task incomplete; independent tasks with satisfied dependencies may proceed. Preparing a PR requires C3 and the applicable evidence; merging additionally requires final CI and human review under AGENTS.

## TDD execution

Use the installed [TDD skill](../../.agents/skills/tdd/SKILL.md). SPEC's [testing decisions](./SPEC.md#testing-decisions) and task gates provide the existing public boundaries. At C0, cite the approved seam; seek maintainer agreement only when adding or materially changing one.

1. Select one observable behavior and derive its expected result from the contract or a known fixture.
2. Write one test through that public boundary. Establish the harness/minimal interface first if needed, then run the test and record the intended failing assertion. Obtain C1 release when required.
3. Implement only enough behavior to pass. Run the test and relevant regression checks, record green and return at C2.
4. Repeat for normal and required failure paths, one vertical slice at a time. Add targeted race/interruption tests for capture, retries, deletion, provider selection, migrations and concurrency; validate both sides of schema/protocol changes.
5. At C3, review test sensitivity as well as code. Refactoring belongs here; the assigned executor performs it and reruns checks before renewed review.

Use real temporary SQLite, filesystem and subprocess contracts where durability depends on them, plus the pinned vector store for its integration gates. Mock external providers deterministically at the boundary. Assertions must establish Locus behavior beyond merely echoing a mock result. Gemini follows the same red/green evidence workflow for critical frontend interactions, using impeccable.

Documentation requires link/consistency checks. Initial scaffolding requires a working test command before behavioral TDD can begin. Feasibility experiments and manual hardware checks use recorded hypotheses, commands, environment and measured outcomes when automated red is unavailable. Record these exceptions at C0; production behavior still needs its automatable contracts tested. Never report unavailable checks as passing.

## Escalation and resumption

Stop the affected slice and return evidence when a contract is contradictory, a dependency fails, ownership overlaps, a race/durability assumption is unresolved, or two attempted fixes fail to resolve the same defect. Include the reproducer, attempted fixes, current diff and smallest unresolved question. Escalate immediately for potential data loss or unauthorized content egress.

The orchestrator may split the slice or assign the unresolved backend work to Astra low / Sol high within the approved routing policy. Record the new owner and reason before continuation. Model escalation cannot solve missing hardware, account access or maintainer authority; record those blockers and continue independent authorized work. Contract changes require synchronized governing documents before affected implementation resumes.

Allow parallel execution only with settled contracts and disjoint file ownership, using isolated worktrees where appropriate. One owner coordinates shared schema/protocol revisions; inspect the integrated result and rerun affected producer/consumer checks before acceptance.

On resume, read AGENTS, this guide and the task record, inspect the actual worktree and compare it with the recorded revision. Reconcile in-flight executors and ownership before dispatching replacements. Reuse valid evidence, rerun checks invalidated by changes, and resume at the first unsatisfied checkpoint. Acceptance never follows merely from a previous session saying it finished.
