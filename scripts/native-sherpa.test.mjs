import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { sherpaArchive, sherpaEnv, sherpaVersion } from "./native-sherpa.mjs";

async function checkout(t) {
  const root = await mkdtemp(join(tmpdir(), "monocode-sherpa-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  await writeFile(
    join(root, "Cargo.lock"),
    '[[package]]\nname = "sherpa-onnx-sys"\nversion = "1.13.8"\n',
  );
  return root;
}

test("names the static build sherpa-onnx-sys unpacks for each platform", () => {
  assert.equal(sherpaArchive("1.13.8", "win32", "x64"), "sherpa-onnx-v1.13.8-win-x64-static-MT-Release-lib");
  assert.equal(sherpaArchive("1.13.8", "darwin", "arm64"), "sherpa-onnx-v1.13.8-osx-arm64-static-lib");
  assert.equal(sherpaArchive("1.13.8", "linux", "arm64"), "sherpa-onnx-v1.13.8-linux-aarch64-static-lib");
  assert.equal(sherpaArchive("1.13.8", "freebsd", "x64"), undefined);
});

test("reuses the libraries a previous build already fetched", async (t) => {
  const root = await checkout(t);
  assert.equal(sherpaVersion(root), "1.13.8");
  const options = { platform: "win32", arch: "x64", roots: [root] };
  const env = { PATH: "bin" };
  assert.equal(sherpaEnv(root, env, options), env, "nothing on disk: sherpa downloads them");
  const lib = join(root, "target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-win-x64-static-MT-Release-lib/lib");
  await mkdir(lib, { recursive: true });
  assert.deepEqual(sherpaEnv(root, env, options), { PATH: "bin", SHERPA_ONNX_LIB_DIR: lib });
  const chosen = { SHERPA_ONNX_LIB_DIR: "elsewhere" };
  assert.equal(sherpaEnv(root, chosen, options), chosen, "a folder set by hand wins");
});

test("a checkout without the native lockfile leaves the environment alone", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "monocode-sherpa-empty-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  assert.equal(sherpaVersion(root), undefined);
  const env = { PATH: "bin" };
  assert.equal(sherpaEnv(root, env, { roots: [root] }), env);
});
