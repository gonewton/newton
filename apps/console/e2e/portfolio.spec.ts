import { test, expect } from '@playwright/test';

test('portfolio page renders', async ({ page }) => {
  await page.goto('/portfolio');
  await expect(page).toHaveTitle(/Newton Console/);
});
