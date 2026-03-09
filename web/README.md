# OctoRill Web UI

`web/` contains the React + Vite frontend and the Storybook used for page-state and UI regression review.

## Commands

### App development

```bash
bun install
bun run dev
```

Open `http://127.0.0.1:55174`.

### App preview build

```bash
bun run build
bun run preview
```

Preview runs on `http://127.0.0.1:55175`.

### Storybook development

```bash
bun run storybook
```

Open `http://127.0.0.1:55176`.

### Storybook static build

```bash
bun run storybook:build
```

The static output is written to `web/storybook-static/` and is later assembled into the public docs site under `/storybook/`; the primary docs navigation links directly to `/storybook/index.html`, while `/storybook.html` remains available as a curated hub page.

## Storybook scope

The current public-facing Storybook coverage focuses on:

- `Pages/*`: landing and dashboard flows.
- `Admin/*`: user management, jobs center, and task detail states.
- `Layout/*`: footer metadata fallback states.
- `UI/*`: reusable primitives and foundational components.

When adding new stories, prefer realistic mocked states and docs descriptions so the docs view stays readable without opening Canvas first.
