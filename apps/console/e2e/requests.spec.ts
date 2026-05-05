import { test, expect } from '@playwright/test';

test('requests page renders', async ({ page }) => {
  await page.goto('/requests');
  await expect(page).toHaveTitle(/Newton Console/);
});
