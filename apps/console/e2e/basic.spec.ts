import { test, expect } from '@playwright/test';

test('basic page renders', async ({ page }) => {
  await page.goto('/');
  await expect(page).toHaveTitle(/Newton Console/);
});
