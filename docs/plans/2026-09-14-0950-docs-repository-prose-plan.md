---
title: Repository Prose Alignment - Plan
type: docs
date: 2026-09-14
topic: repository-prose-alignment
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-plan-bootstrap
execution: code
---

# Repository Prose Alignment - Plan

## Goal Capsule

- **Objective:** Maintainers and v0.1.0 users can rely on Cairn.md's repository text to describe the shipped product, its limitations, and its verification requirements accurately.
- **Means:** Apply a bounded prose cleanup across documentation, package metadata, user-facing fallback copy, and stale test commentary (KTD1).
- **Authority:** Shipped behavior and executable verification contracts take precedence over promotional wording; the current product-scope plan remains a historical implementation record.
- **Execution profile:** Code.
- **Stop conditions:** Stop if a wording change would alter product behavior or require a new product decision.
- **Tail ownership:** LFG owns review, verification, push, pull-request creation, and CI follow-through.

## Product Contract

### Summary

Align repository prose with Cairn.md v0.1.0 without changing the application's feature scope.

### Problem Frame

Several repository passages used broad performance claims, stale prerelease language, obsolete implementation paths, or dense sentences that obscured the actual behavior. The desktop-service fallback also described the failure without telling the user how to recover.

### Requirements

**Product and release documentation**

- R1. Public repository text must describe Cairn.md v0.1.0 and defer measurable performance claims to `docs/PERFORMANCE.md`.
- R2. Release and support guidance must describe the current prerelease without implying that no build exists.
- R3. The retained product-scope plan must identify itself as a historical v0.1.0 implementation record, distinguish later reference corrections from original decisions, and use current migration and component paths.

**Application and package copy**

- R4. Package descriptions must state Cairn.md's purpose without unsupported speed or resource claims.
- R5. The startup failure screen must explain that Cairn.md could not finish starting, preserve a specific error when available, and always give text-only restart guidance.
- R6. Test commentary must describe the present test boundary without claiming that desktop harness infrastructure is absent.

### Scope Boundaries

**In scope**

- Existing prose in `README.md`, `docs/`, package metadata, the desktop-service fallback, and the affected integration-test comment.
- Verification that the wording-only changes leave the TypeScript application and focused editor integration tests healthy.
- Repository-wide discovery for stale prerelease wording, unsupported performance claims, obsolete paths, and superseded terminology, with any exclusions justified against current behavior.

**Outside this change**

- New Cairn.md features, roadmap changes, broad code refactors, installer publication, and changes beyond the reviewed prose findings.
- Interactive retry or relaunch controls on the startup failure screen.

## Planning Contract

### Key Technical Decisions

- KTD1. Treat executable behavior and dedicated contracts as the source of truth, then replace only wording that is stale, vague, unsupported, or difficult to scan. This keeps the change reviewable and avoids inventing product behavior.

### Assumptions

- The current branch's reviewed change set is the intended implementation baseline.
- Existing v0.1.0 behavior, roadmap decisions, and release requirements remain unchanged.
- Full installer validation is not required for wording-only changes, but Cargo and Tauri configuration must still parse and build.

## Implementation Units

### U1. Align repository and product documentation

- **Goal:** Make maintained documentation accurate, current, and easier to scan.
- **Requirements:** R1-R3.
- **Dependencies:** None.
- **Files:** `README.md`, `docs/ARCHITECTURE.md`, `docs/PERFORMANCE.md`, `docs/RELEASE.md`, `docs/USER_GUIDE.md`, `docs/plans/2026-09-08-1749-feat-cairn-md-product-scope-plan.md`.
- **Approach:** Apply KTD1. Replace unsupported claims with links to measured contracts, correct stale release and implementation references, preserve the historical plan's decisions, label later reference corrections in its status banner, and split dense operational passages without dropping constraints.
- **Patterns to follow:** Existing repository terminology and the concrete verification contracts in `docs/PERFORMANCE.md` and `docs/RELEASE.md`.
- **Test scenarios:** Test expectation: none -- this unit changes documentation without changing executable behavior.
- **Verification:** A repository-wide wording scan and final diff show that every edited passage retains its original facts or corrects them against current repository evidence; links and paths resolve to existing files, and any retained search match is justified by context.

### U2. Align user-facing fallback and package copy

- **Goal:** Ensure application and package copy makes concrete, supportable claims, and test commentary reflects the current test boundary.
- **Requirements:** R4-R6.
- **Dependencies:** U1.
- **Files:** `src/app/App.tsx`, `src/app/App.test.tsx`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `tests/e2e/editor-modes.spec.ts`.
- **Approach:** Apply KTD1. Remove unsupported performance adjectives, present startup failure copy as a summary followed by available diagnostic detail and restart guidance, and update the integration-test boundary comment. Keep the recovery action instructional; do not add retry or relaunch behavior.
- **Test scenarios:**
  - When startup fails without a specific error, the fallback explains that Cairn.md could not finish starting and tells the user to restart the installed Windows application.
  - When startup returns an error message, the fallback shows that diagnostic detail after the summary and still shows the restart guidance.
  - When a restart succeeds, Cairn.md follows its normal startup flow; when it fails again, the same fallback remains available with the new diagnostic detail.
  - Existing Visual and Source mode integration tests continue to exercise the same session and parser behavior.
- **Verification:** Type checking, linting, focused startup-failure component coverage, the editor-mode integration suite, Cargo validation, and a Tauri debug build without bundling pass with no runtime feature changes.

## Verification Contract

- `npm run typecheck` must complete without TypeScript errors.
- `npm run lint` must complete with zero warnings.
- `npm test -- --run src/app/App.test.tsx` must prove both startup-failure copy variants.
- `npm test -- --run tests/e2e/editor-modes.spec.ts` must pass all focused editor-mode scenarios.
- `cargo check --manifest-path src-tauri/Cargo.toml` must validate the Rust manifest and desktop crate.
- `npm run tauri build -- --debug --no-bundle` must validate Tauri configuration and build the desktop application without producing an installer.
- Search tracked repository text for stale prerelease wording, unsupported performance claims, obsolete implementation paths, and superseded terminology; inspect every match rather than treating the query as proof by itself.
- Review the final diff against `origin/main` to confirm that it contains only the scoped text, metadata, fallback-copy, test-comment, and plan changes.

## Definition of Done

- R1-R6 are satisfied by the final diff.
- U1 and U2 verification outcomes pass.
- No product behavior, roadmap commitment, or release threshold changes unintentionally.
- No abandoned or experimental changes remain in the branch.
