# Playwright × Ark UI: option lists live in a Portal

Ark UI's `Select`, `Combobox`, `Menu`, and similar overlay primitives render
their option list into a `<Portal>` — a sibling of the trigger, attached
high up in the DOM (typically at the document body), not inside the
trigger's React subtree.

This breaks the natural Playwright pattern of scoping queries to the
trigger:

```ts
// ❌ Won't find anything — options are not inside `trigger`'s subtree.
const trigger = page.getByRole('combobox', { name: /project/i })
await trigger.click()
await trigger.getByRole('option', { name: '[seed] Secondary' }).click()
```

Use page-scoped role selectors instead, after opening the menu:

```ts
// ✅ Page-scoped — finds the option in the Portal.
await page.getByRole('combobox', { name: /project/i }).click()
await page.getByRole('option', { name: '[seed] Secondary' }).click()
```

This applies to every Ark UI primitive that opens an overlay
(`Select.Content`, `Combobox.Content`, `Menu.Content`, `Popover.Content`,
`Dialog.Content`, etc.) when wrapped in `<Portal>` — which is the default
in our wrappers (see `web/src/components/ProjectSwitcher.tsx`).

The native `<select>` used by `LanguageSwitcher` does NOT have this
problem; `selectOption('ja')` works directly because there is no Portal.

## Reference

- `web/tests/e2e/specs/02-project-switching.spec.ts` is the canonical
  example.
- Ark UI source: every overlay primitive uses `Portal` so options can
  visually escape ancestors with `overflow: hidden` / clipping
  containers. See `@ark-ui/react/portal`.
