import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/?mock=1", { waitUntil: "load" });
  await page.waitForTimeout(1500);
});

test("shell landmarks and nav", async ({ page }) => {
  await expect(page.locator("header.topbar")).toBeVisible();
  await expect(page.locator("nav.rail")).toBeVisible();
  await expect(page.locator("footer.statusbar")).toBeVisible();
  await expect(page.getByRole("link", { name: "Skip to content" })).toHaveCount(1);
  await expect(page.locator('.rail-item[aria-current="page"]')).toHaveText(/Console/);
  await expect(page.getByRole("heading", { name: "Living room PS5" })).toBeVisible();
});

test("library page lists apps, images and packages", async ({ page }) => {
  await page.getByRole("button", { name: "Library", exact: true }).click();
  await expect(page.locator(".tile")).toHaveCount(7);
  await expect(page.locator(".tile .t").first()).toContainText(/Demon's Souls/i);
});

test("library filters by system", async ({ page }) => {
  await page.getByRole("button", { name: "Library", exact: true }).click();
  await page.getByRole("tab", { name: "PS4" }).click();
  await expect(page.locator(".tile")).toHaveCount(1);
  await expect(page.locator(".tile .t").first()).toContainText(/The Last of Us Part II/i);
});

test("game index wires the search source", async ({ page }) => {
  await page.getByRole("button", { name: "Game Index" }).click();
  await expect(page.getByText("God of War Ragnarok").first()).toBeVisible();
  await expect(page.getByText("Elden Ring").first()).toBeVisible();
});

test("downloads page lists local files", async ({ page }) => {
  await page.getByRole("button", { name: "Downloads" }).click();
  await expect(page.getByText("God of War Ragnarok.exfat")).toBeVisible();
  await expect(page.getByRole("button", { name: "Send to PS5" }).first()).toBeVisible();
});

test("payload catalogue renders with send buttons", async ({ page }) => {
  await page.getByRole("navigation", { name: "Sections" }).getByRole("button", { name: "Payloads" }).click();
  await expect(page.locator(".store-card")).toHaveCount(8);
  await expect(page.locator(".store-card .name").filter({ hasText: "Ghostpad" })).toBeVisible();
});

test("controller exposes the pad and combos", async ({ page }) => {
  await page.getByRole("button", { name: "Controller" }).click();
  await expect(page.getByRole("heading", { name: "Virtual controller" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Cross", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Connect pad" }).first()).toBeVisible();
  await expect(page.getByRole("button", { name: "L3 + R3" })).toBeVisible();
});

test("controller connects and reports state", async ({ page }) => {
  await page.getByRole("button", { name: "Controller" }).click();
  await page.getByRole("button", { name: "Connect pad" }).first().click();
  await expect(page.getByText("connected to Living room PS5")).toBeVisible();
  await expect(page.getByRole("button", { name: "Disconnect" })).toBeVisible();
});

test("conversions page explains the formats", async ({ page }) => {
  await page.getByRole("button", { name: "Convert" }).click();
  await expect(page.getByText("Folder to ISO")).toBeVisible();
  await expect(page.getByText("Pack ZIP", { exact: true })).toBeVisible();
  await expect(page.getByText("Merge parts")).toBeVisible();
});

test("inspector queue shows the seeded jobs", async ({ page }) => {
  await expect(page.locator(".queue-card .job-row")).toHaveCount(2);
});

test("settings exposes config controls", async ({ page }) => {
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByText("Game search backend").first()).toBeVisible();
  await expect(page.getByText("Homebrew folder on the console")).toBeVisible();
  await expect(page.getByText("Payload catalogue URL")).toBeVisible();
});

test("devices page exposes the service ports", async ({ page }) => {
  await page.getByRole("button", { name: "Devices" }).click();
  await page.getByRole("button", { name: "Add console" }).click();
  await expect(page.getByText("Ghostpad port")).toBeVisible();
  await expect(page.getByText("Payload loader port")).toBeVisible();
});

test("unslop-anything a11y pass: search inputs have accessible names", async ({ page }) => {
  await page.getByRole("button", { name: "Library", exact: true }).click();
  await expect(page.getByRole("textbox", { name: "Filter by title" })).toBeVisible();
  await page.getByRole("button", { name: "Game Index" }).click();
  await expect(page.getByRole("textbox", { name: "Search games" })).toBeVisible();
});
