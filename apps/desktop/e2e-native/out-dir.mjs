// Where a native run keeps what it leaves: logs, screenshots, videos,
// results.json (merged from run to run) and, for the Claude proxy, the spend
// ledger that a budget is counted against across restarts. So it has to be the
// same folder every time, which is why this is not a fresh `mkdtemp` per run.
//
// `OUT` names it. With none, it is `edytlab-native-<uid>` in the temp folder,
// made owner-only, and refused if something else already holds the name (a
// folder or a link another user put there), because a shared folder with a
// fixed name is somewhere another user can plant a link (S5443).
import { lstatSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export function outDir(env = process.env) {
  if (env.OUT) return env.OUT;
  const uid = typeof process.getuid === "function" ? process.getuid() : undefined;
  const dir = join(tmpdir(), `edytlab-native-${uid ?? "user"}`);
  mkdirSync(dir, { recursive: true, mode: 0o700 });
  const found = lstatSync(dir);
  if (!found.isDirectory() || (uid !== undefined && found.uid !== uid)) {
    throw new Error(`${dir} is not a folder of yours; set OUT to a folder you own`);
  }
  return dir;
}
