import fs from "node:fs";
import { pathToFileURL } from "node:url";
import { chromium } from "/opt/fudian-playwright/node_modules/playwright/index.mjs";

const [sourcePath, screenshotPath] = process.argv.slice(2);
if (!sourcePath || !screenshotPath) {
  throw new Error("inspect.mjs requires source and screenshot paths");
}

const browser = await chromium.launch({ headless: true, args: ["--no-sandbox"] });
try {
  const page = await browser.newPage({ viewport: { width: 390, height: 844 } });
  await page.goto(pathToFileURL(sourcePath).href, { waitUntil: "load" });
  await page.screenshot({ path: screenshotPath, fullPage: true });
  const result = {
    title: await page.title(),
    text: (await page.locator("body").innerText()).slice(0, 4096),
    viewport: page.viewportSize(),
    screenshotBytes: fs.statSync(screenshotPath).size,
  };
  process.stdout.write(JSON.stringify(result));
} finally {
  await browser.close();
}
