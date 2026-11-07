import { chromium } from "@playwright/test";

const browser = await chromium.launch({ channel: "msedge" });
const page = await browser.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(`PAGEERROR: ${e.message}\n${e.stack}`));
page.on("console", (m) => {
  if (m.type() === "error" || m.type() === "warning")
    errors.push(`${m.type().toUpperCase()}: ${m.text()}`);
});
page.on("requestfailed", (r) =>
  errors.push(`REQFAIL: ${r.url()} ${r.failure()?.errorText}`),
);
page.on("framenavigated", (f) => {
  if (f === page.mainFrame()) errors.push(`NAV -> ${f.url()}`);
});

try {
  await page.goto("http://localhost:1420", { timeout: 30000 });
} catch (e) {
  errors.push(`GOTO: ${e.message}`);
}
await page.waitForTimeout(6000);

const safe = async (fn, fallback) => {
  try {
    return await fn();
  } catch (e) {
    return `(eval failed: ${e.message.split("\n")[0]})`;
  }
};

console.log("URL:", await safe(() => page.url(), "?"));
console.log("=== ERRORS/EVENTS ===");
console.log(errors.length ? errors.join("\n---\n") : "(none)");
console.log("=== BODY TEXT ===");
console.log(
  await safe(
    () => page.evaluate(() => document.body.innerText.slice(0, 500)),
    "?",
  ),
);
console.log("=== ROOT HTML (first 300) ===");
console.log(
  await safe(
    () =>
      page.evaluate(
        () => document.getElementById("root")?.innerHTML.slice(0, 300) ?? "NO #root",
      ),
    "?",
  ),
);
await page.screenshot({ path: "scripts/_blank-check.png" }).catch(() => {});
await browser.close();
