# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

OctoRill serves individual GitHub users who need to keep up with activity that directly affects them: releases from watched or starred projects, stars on their own repositories, new followers, daily summaries, and GitHub Inbox context.

The primary usage context is repeated reading rather than one-off administration. Users open the app to answer "what changed, what matters, and where should I go next" without jumping across repository pages, release pages, notifications, and translation tools.

Admin users also maintain sync, translation, scheduling, LLM runtime, and user-management health. These admin surfaces support operations and observability, not the ordinary reading path.

## Product Purpose

OctoRill is a personal GitHub activity workspace. It does not replace GitHub. It collects the parts of GitHub activity that are most useful from the user's own perspective, then makes them easier to scan, translate, summarize, revisit, and route back to GitHub when full context is needed.

Success means the user can quickly understand the current window of activity, read release content in the most useful lane, review a stable daily brief, notice direct social feedback, and jump to GitHub only when the original workflow belongs there.

## Positioning

OctoRill is a personal reading workspace for the GitHub activity that matters to one user. It improves continuity across releases, social feedback, daily briefs, and Inbox context while leaving GitHub as the source of truth and destination for workflows it owns.

## Operating Context

The primary loop is a repeated Dashboard reading session: a user scans the current activity window, changes between reading lanes, checks a daily brief, and follows an original GitHub link when deeper context or action is needed. Admins separately operate synchronization, translation, scheduling, LLM runtime, and user-management health.

## Capabilities and Constraints

The product collects release activity, direct social feedback, daily summaries, and GitHub Inbox context. It supports translation and polished reading lanes but does not become a full GitHub client or take over GitHub-native workflows. Product UI is Chinese-first; detailed behavior, permissions, and data semantics remain in `docs/product.md`.

## Brand Commitments

Precise, calm, and companionable.

The interface should feel like a focused reading instrument with enough warmth to make recurring personal use pleasant. It should be technically credible, restrained under load, and clear about what it does not own.

## Evidence on Hand

- `docs/product.md` is the internal reference for product semantics and implementation boundaries.
- `web/src/` and the existing Storybook stories provide implemented UI examples for the reading and operational surfaces.

## Product Principles

1. Preserve personal context: every surface should make it clear why this item is relevant to the current user.
2. Let reading lead: prioritize scanability, stable grouping, and lane switching over decorative density.
3. Keep GitHub as the source of truth: provide clear paths back to GitHub instead of pretending every workflow belongs in OctoRill.
4. Separate reading from operations: admin observability should be powerful, but it must not leak into the ordinary user's work surface.
5. Make summaries accountable: translated, polished, and briefed content should help comprehension while keeping original material close enough to verify.

## Accessibility & Inclusion

Target WCAG 2.2 AA for product-critical surfaces. Keyboard navigation, visible focus states, readable contrast in light and dark themes, reduced-motion compatibility, and robust responsive layouts are baseline requirements.

The product's current language posture is Chinese-first. UI copy should stay concise, concrete, and consistent across Dashboard, release detail, daily brief, settings, and admin surfaces.
