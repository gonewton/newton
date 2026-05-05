import { test, expect } from '@playwright/test';

test('parity-smoke page renders', async ({ page }) => {
  await page.goto('/');
  await expect(page).toHaveTitle(/Newton Console/);
});
