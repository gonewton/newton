import { test, expect } from '@playwright/test';

test('monitor page renders', async ({ page }) => {
  await page.goto('/monitor');
  await expect(page).toHaveTitle(/Newton Console/);
});
