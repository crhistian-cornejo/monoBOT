// Runs cargo from the repository root with the speech engine's libraries
// found (see native-sherpa.mjs): `node scripts/native-cargo.mjs test ...`.
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { sherpaEnv } from "./native-sherpa.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const result = spawnSync("cargo", process.argv.slice(2), {
  cwd: root,
  env: sherpaEnv(root),
  stdio: "inherit",
});
if (result.error) console.error(result.error.message);
process.exit(result.status ?? 1);
