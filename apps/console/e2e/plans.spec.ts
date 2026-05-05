import { test, expect } from '@playwright/test';

test('plans page renders', async ({ page }) => {
  await page.goto('/plans');
  await expect(page).toHaveTitle(/Newton Console/);
});
