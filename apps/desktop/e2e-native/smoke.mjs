import { Driver } from "./webdriver.mjs";
import { outDir } from "./out-dir.mjs";
const app = process.env.APP ?? "/home/user/edytlab/target/debug/edytlab-desktop";
const out = outDir();
const d = await Driver.start({ application: app });
try {
  await new Promise((r) => setTimeout(r, 4000));
  console.log("title:", await d.cmd("GET", "/title"));
  console.log("body:", (await d.exec(() => document.body.innerText)).slice(0, 400));
  await d.screenshot(`${out}/00-first-launch.png`);
} finally {
  await d.quit();
}
