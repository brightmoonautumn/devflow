# Beautiful UI source components

These components are copied from the source snippets embedded in the official
[Beautiful UI gallery](https://www.beautifului.dev/) on 2026-10-19, under its
[MIT license](https://www.beautifului.dev/license). See LICENSE in this directory.
This is source integration, not an npm package named Beautiful UI.

- `task-rows.tsx`: official Task Rows, adapted to data-driven project states,
  optional icons, paths, status captions, always-visible chart content, action slots, and accessible metadata expansion.
  Removed the sample data, scripted timers and automatic failure/success cycle.
- `search.tsx`: official Search (`SearchList`), adapted to a controlled query and
  input-only mode for filtering the workspace list. The optional result list uses
  native buttons instead of the gallery-only GlideMenu dependency.
- `theme.css`: official light/dark color tokens and component animations, scoped
  to the workspace, plus the Tailwind CSS 4 theme mapping. Tailwind preflight is
  intentionally omitted so it does not reset other feature pages.

Business data and actions are supplied by `features/workspace/WorkspacePage.tsx`.
Do not add a gallery demo loop to live process or project states.
