import { build } from "esbuild";
import { copyFile, rm, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

// Paths below are relative to this folder, whatever the caller's directory.
process.chdir(dirname(fileURLToPath(import.meta.url)));
// esbuild only writes the legal file when the bundle has license comments;
// never let package.mjs ship one left over from an earlier build.
await rm("build/host/monocode-host.mjs.LEGAL.txt", { force: true });

const result = await build({
  entryPoints: ["cli.ts"],
  outfile: "build/host/monocode-host.mjs",
  bundle: true,
  platform: "node",
  format: "esm",
  target: "node24",
  // windows.ts embeds the shared Windows ACL script.
  loader: { ".ps1": "text" },
  minify: true,
  treeShaking: true,
  // Third-party license comments go to monocode-host.mjs.LEGAL.txt, which
  // package.mjs ships beside the bundle.
  legalComments: "external",
  // Error stacks from a packaged host map back through the sourcemap.
  sourcemap: true,
  // build/host/meta.json lists every source the bundle reaches; inspect it
  // with https://esbuild.github.io/analyze/ before adding or pruning files.
  metafile: true,
});
await writeFile("build/host/meta.json", JSON.stringify(result.metafile));
await copyFile("provider-guard.mjs", "build/host/provider-guard.mjs");
