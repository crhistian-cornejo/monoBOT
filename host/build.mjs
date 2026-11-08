import { build } from "esbuild";
import { copyFile } from "node:fs/promises";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

// Paths below are relative to this folder, whatever the caller's directory.
process.chdir(dirname(fileURLToPath(import.meta.url)));

await build({
  entryPoints: ["cli.ts"],
  outfile: "build/host/monocode-host.mjs",
  bundle: true,
  platform: "node",
  format: "esm",
  target: "node24",
  loader: { ".ps1": "text", ".md": "text" },
  define: { "import.meta.hot": "undefined" },
  sourcemap: true,
});
await copyFile("provider-guard.mjs", "build/host/provider-guard.mjs");
