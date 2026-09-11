import { expect, test } from "@playwright/test";

test("applying defaults preserves a new scene draft and the sample wallpaper has no UI art", async ({ page }) => {
  await page.setViewportSize({ width: 1180, height: 950 });
  await page.goto("/iframe.html?id=settings-petscenestudio--preview-and-failure&viewMode=story");
  await page.getByRole("button", { name: "管理 奶糖", exact: true }).click();
  await page.getByRole("button", { name: "新增场景", exact: true }).click();
  const field = page.getByRole("textbox", { name: "新场景名称", exact: true });
  await field.fill("窗边草稿");
  await page.getByRole("button", { name: "应用宠物默认配置", exact: true }).click();
  await expect(field).toBeVisible();
  await expect(field).toHaveValue("窗边草稿");
  await expect(page.locator(".pet-scene-wallpaper").first()).toHaveAttribute("src", /pet-room/);
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.getByRole("heading", { name: "窗边草稿", exact: true })).toBeVisible();
  await expect(field).toHaveCount(0);
});
