import { test, expect } from '@playwright/test';

test('editor page renders', async ({ page }) => {
  await page.goto('/editor');
  await expect(page).toHaveTitle(/Newton Console/);
});
