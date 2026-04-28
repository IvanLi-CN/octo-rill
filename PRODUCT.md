# Product

## Register

product

## Users

OctoRill serves individual GitHub users who need to keep up with activity that directly affects them: releases from watched or starred projects, stars on their own repositories, new followers, daily summaries, and GitHub Inbox context.

The primary usage context is repeated reading rather than one-off administration. Users open the app to answer "what changed, what matters, and where should I go next" without jumping across repository pages, release pages, notifications, and translation tools.

Admin users also maintain sync, translation, scheduling, LLM runtime, and user-management health. These admin surfaces support operations and observability, not the ordinary reading path.

## Product Purpose

OctoRill is a personal GitHub activity workspace. It does not replace GitHub. It collects the parts of GitHub activity that are most useful from the user's own perspective, then makes them easier to scan, translate, summarize, revisit, and route back to GitHub when full context is needed.

Success means the user can quickly understand the current window of activity, read release content in the most useful lane, review a stable daily brief, notice direct social feedback, and jump to GitHub only when the original workflow belongs there.

## Brand Personality

Precise, calm, and companionable.

The interface should feel like a focused reading instrument with enough warmth to make recurring personal use pleasant. It should be technically credible, restrained under load, and clear about what it does not own.

## Anti-references

OctoRill should not look or behave like a full GitHub clone, a generic AI news dashboard, a metrics-first admin console, or a noisy social feed.

Avoid designs that make releases feel like disposable notifications, bury original GitHub context, over-celebrate AI output, or turn operational tools into the main product story.

## Design Principles

1. Preserve personal context: every surface should make it clear why this item is relevant to the current user.
2. Let reading lead: prioritize scanability, stable grouping, and lane switching over decorative density.
3. Keep GitHub as the source of truth: provide clear paths back to GitHub instead of pretending every workflow belongs in OctoRill.
4. Separate reading from operations: admin observability should be powerful, but it must not leak into the ordinary user's work surface.
5. Make summaries accountable: translated, polished, and briefed content should help comprehension while keeping original material close enough to verify.

## Accessibility & Inclusion

Target WCAG 2.2 AA for product-critical surfaces. Keyboard navigation, visible focus states, readable contrast in light and dark themes, reduced-motion compatibility, and robust responsive layouts are baseline requirements.

The product's current language posture is Chinese-first. UI copy should stay concise, concrete, and consistent across Dashboard, release detail, daily brief, settings, and admin surfaces.
