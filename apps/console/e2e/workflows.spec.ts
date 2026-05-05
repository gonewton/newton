import { test, expect } from '@playwright/test';

test('workflows page renders', async ({ page }) => {
  await page.goto('/workflows');
  await expect(page).toHaveTitle(/Newton Console/);
});
