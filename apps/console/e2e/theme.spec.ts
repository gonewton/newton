import { test, expect } from '@playwright/test';

test('theme page renders', async ({ page }) => {
  await page.goto('/theme');
  await expect(page).toHaveTitle(/Newton Console/);
});
