---
name: OctoRill
description: A calm personal GitHub activity workspace for reading releases, briefs, social feedback, and inbox context.
colors:
  paper-bg: "oklch(0.985 0.01 95)"
  ink: "oklch(0.18 0.02 70)"
  paper-card: "oklch(0.995 0.008 95)"
  muted-paper: "oklch(0.965 0.01 95)"
  muted-ink: "oklch(0.45 0.03 70)"
  warm-border: "oklch(0.9 0.01 95)"
  focus-ring: "oklch(0.7 0.05 240)"
  destructive: "oklch(0.577 0.245 27.325)"
  brand-slate: "#495675"
  dark-bg: "oklch(0.145 0 0)"
  dark-card: "oklch(0.205 0 0)"
  dark-ink: "oklch(0.985 0 0)"
typography:
  display:
    fontFamily: "IBM Plex Sans SC, ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif"
    fontSize: "clamp(2.25rem, 5vw, 3.75rem)"
    fontWeight: 600
    lineHeight: 0.95
    letterSpacing: "normal"
  headline:
    fontFamily: "IBM Plex Sans SC, ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif"
    fontSize: "2rem"
    fontWeight: 600
    lineHeight: 1.1
    letterSpacing: "normal"
  title:
    fontFamily: "IBM Plex Sans SC, ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "normal"
  body:
    fontFamily: "IBM Plex Sans SC, ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif"
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.75
    letterSpacing: "normal"
  label:
    fontFamily: "IBM Plex Sans SC, ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 500
    lineHeight: 1.25
    letterSpacing: "normal"
  mono:
    fontFamily: "IBM Plex Mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, Liberation Mono, Courier New, monospace"
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: 1.5
    letterSpacing: "normal"
rounded:
  xs: "2px"
  sm: "10px"
  md: "12px"
  lg: "14px"
  xl: "18px"
  card: "24px"
  hero: "32px"
  pill: "9999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
  section: "40px"
components:
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.paper-bg}"
    typography: "{typography.label}"
    rounded: "{rounded.md}"
    padding: "8px 16px"
    height: "36px"
  button-secondary:
    backgroundColor: "{colors.muted-paper}"
    textColor: "{colors.ink}"
    typography: "{typography.label}"
    rounded: "{rounded.md}"
    padding: "8px 16px"
    height: "36px"
  input-default:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "4px 12px"
    height: "36px"
  card-default:
    backgroundColor: "{colors.paper-card}"
    textColor: "{colors.ink}"
    rounded: "{rounded.xl}"
    padding: "24px"
  badge-default:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.paper-bg}"
    typography: "{typography.mono}"
    rounded: "{rounded.pill}"
    padding: "2px 8px"
---

# Design System: OctoRill

## 1. Overview

**Creative North Star: "The Personal Reading Desk"**

OctoRill should feel like a quiet desk where the user's own GitHub world has already been sorted into readable piles. The physical scene is a developer opening the dashboard during a morning review or late focus session, scanning release context under normal room light, and wanting confidence without ceremony.

The system is product-first: dense enough for repeated reading, warm enough for daily use, and disciplined enough to keep GitHub as the source of truth. Light mode uses paper-like warmth; dark mode is available for low-light reading, but it must not become a neon observability console.

It explicitly rejects the PRODUCT.md anti-references: a full GitHub clone, a generic AI news dashboard, a metrics-first admin console, and a noisy social feed.

**Key Characteristics:**
- Warm paper surfaces with restrained ink-first contrast.
- Rounded product primitives, with larger radii only for landing and shell surfaces.
- Sparse accent color, mostly reserved for focus, status, brand, and admin affordances.
- Reading layouts that prefer stable grouping, sidebars, tabs, and accountable summaries.

## 2. Colors

The palette is restrained: warm neutrals carry the interface, ink defines action, and blue-slate appears only where brand or focus needs a sharper signal.

### Primary
- **Warm Ink** (`paper-bg` / `ink` pairing): the default action and text system. It makes primary actions feel decisive without importing a generic blue SaaS tone.
- **Brand Slate** (`brand-slate`): reserved for avatar fallback, admin hints, and brand-adjacent identity moments. It must remain rare.

### Secondary
- **Quiet Paper** (`muted-paper`): tabs, secondary buttons, inactive controls, empty surfaces, and low-pressure status areas.
- **Focus Blue** (`focus-ring`): keyboard focus and ring states. It is functional, not decorative.

### Neutral
- **Reading Paper** (`paper-bg`): the light theme page surface.
- **Card Paper** (`paper-card`): cards, popovers, dialogs, and readable containers.
- **Muted Ink** (`muted-ink`): helper text, timestamps, descriptions, and secondary metadata.
- **Warm Border** (`warm-border`): dividers and component outlines.
- **Dark Desk** (`dark-bg`, `dark-card`, `dark-ink`): the low-light reading mode, not a separate visual brand.

### Named Rules
**The Ink Leads Rule.** Primary actions use ink on paper. Do not replace core product actions with saturated category colors.

**The Rare Slate Rule.** Brand Slate is an identity accent. If it appears across a full screen as decoration, it is already overused.

## 3. Typography

**Display Font:** IBM Plex Sans SC, with system sans fallbacks
**Body Font:** IBM Plex Sans SC, with system sans fallbacks
**Label/Mono Font:** IBM Plex Mono, with system mono fallbacks

**Character:** The type system is bilingual, sober, and reading-oriented. IBM Plex Sans SC gives Chinese UI copy enough warmth and technical precision, while IBM Plex Mono marks time, counters, hashes, IDs, and operational details.

### Hierarchy
- **Display** (600, `clamp(2.25rem, 5vw, 3.75rem)`, 0.95): landing hero and exceptional empty-state moments only.
- **Headline** (600, `2rem`, 1.1): login cards, major page headers, and top-level panel titles.
- **Title** (600, `1.125rem`, 1.25): card titles, dialog titles, release headings, and admin panel groups.
- **Body** (400, `1rem`, 1.75): markdown prose, release descriptions, daily briefs, and explanatory text. Keep long prose to 65 to 75 characters per line.
- **Label** (500, `0.875rem`, 1.25): buttons, tabs, filters, form labels, and compact navigation.
- **Mono** (500, `0.75rem`, 1.5): dates, IDs, issue numbers, counters, status codes, and compact technical metadata.

### Named Rules
**The Reading First Rule.** Body prose gets line height before decoration. If release text feels compressed, increase breathing room before adding color.

**The Mono Is Evidence Rule.** Use mono for factual identifiers, never for personality.

## 4. Elevation

OctoRill uses a hybrid of tonal layering and restrained shadows. Core product cards are mostly flat with borders and tinted backgrounds; landing panels, floating menus, dialogs, sheets, and hovered release cards may lift. Depth should clarify interaction, not add atmosphere.

### Shadow Vocabulary
- **Primitive Low** (`shadow-xs`, `shadow-sm`): default buttons, tabs, small cards, and control surfaces.
- **Card Hover** (`shadow-md`): release cards and interactive containers on hover only.
- **Landing Panel** (`0 20px 50px rgba(15,23,42,0.07)`): large marketing or unauthenticated panels.
- **Landing Panel Dark** (`0 28px 60px rgba(2,6,23,0.42)`): dark-mode equivalent for large panels.
- **Floating Layer** (`shadow-lg`): dialogs, sheets, popovers, and account menus.

### Named Rules
**The Flat By Default Rule.** Resting product surfaces rely on borders and tonal contrast. Shadows appear for state, focus, or floating layers.

## 5. Components

### Buttons
- **Shape:** gently curved by default (`12px`), with landing CTAs allowed to use softer corners (`16px` to `18px`).
- **Primary:** Warm Ink background with Reading Paper text, medium label type, icon gap (`8px`), default height (`36px`), large CTA height (`48px` to `56px`).
- **Hover / Focus:** hover darkens or tints within the same semantic color. Focus uses the Focus Blue ring (`3px`) and visible border change.
- **Secondary / Ghost / Outline:** secondary uses Quiet Paper, ghost uses no resting fill, outline uses a warm border and only fills on hover.

### Chips
- **Style:** compact rounded pills, usually `12px` or mono `11px` to `12px`, with muted fill and border.
- **State:** selected chips use background and foreground contrast, not colored side stripes.

### Cards / Containers
- **Corner Style:** primitives use `18px`; landing and major shell panels may use `28px` to `32px`.
- **Background:** Card Paper over Reading Paper, often with alpha when layered.
- **Shadow Strategy:** low by default, stronger only for landing panels, popovers, dialogs, and hoverable release cards.
- **Border:** warm border at rest, often reduced opacity (`70%`) for product panels.
- **Internal Padding:** primitive card sections use `24px`; dense feed cards step down to `16px` on mobile.

### Inputs / Fields
- **Style:** transparent background, warm input border, `12px` radius, `36px` height.
- **Focus:** border shifts to Focus Blue and adds a `3px` translucent ring.
- **Error / Disabled:** destructive border and ring for invalid states; disabled controls reduce opacity and remove pointer interaction.

### Navigation
- **Style:** tabs use a soft muted rail with active items returning to the page background. Line tabs use a `2px` underline, not a side stripe.
- **Typography:** labels stay medium weight and compact. Mobile navigation should stay one-line where possible, with icon-only actions for crowded utility controls.
- **States:** active is structural and quiet; hover can increase foreground contrast, but must not add decorative gradients.

### Signature Components
Dashboard release cards are the core product surface. They combine release identity, lane switching, translation state, social activity, reactions, and GitHub escape hatches. Their job is not to impress, but to keep the reading path stable while preserving original context.

## 6. Do's and Don'ts

### Do:
- **Do** keep Warm Ink and Reading Paper as the dominant product contrast.
- **Do** use Storybook states as the source for stable visual verification.
- **Do** keep release content, translation, polished summaries, and original GitHub links close together.
- **Do** use visible focus rings on every interactive primitive.
- **Do** let admin observability surfaces be denser than the ordinary Dashboard, while preserving the same primitives.

### Don't:
- **Don't** make OctoRill look or behave like a full GitHub clone.
- **Don't** turn it into a generic AI news dashboard.
- **Don't** make the admin center a metrics-first admin console that dominates the product story.
- **Don't** let the Dashboard become a noisy social feed.
- **Don't** use colored side-stripe borders, gradient text, default glassmorphism, hero-metric templates, or endless identical card grids.
- **Don't** bury original GitHub context or over-celebrate AI output.
