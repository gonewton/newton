# Testing

This monorepo uses pnpm workspaces. All commands run from the repo root.

## Single install

```sh
pnpm install --frozen-lockfile
```

This installs every workspace (`packages/design-system`, `packages/newton-ui`,
`apps/console`, `examples/mock-server`, `backend`) from one root lockfile.

## Unit tests

```sh
pnpm --filter @newton/design-system test
pnpm --filter newton-ui test
```

## Lint and typecheck

```sh
pnpm run lint
pnpm run typecheck
```

## End-to-end (Playwright)

The console app drives Playwright. The mock server is started automatically by
`playwright.config.ts`.

```sh
pnpm --filter newton-console exec playwright install chromium
pnpm --filter newton-console exec playwright test
```

## Rust tests

The Rust crate is tested separately via:

```sh
./scripts/run-tests.sh
```
