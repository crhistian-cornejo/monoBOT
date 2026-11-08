// sherpa-onnx-sys (dictation's speech engine) downloads its prebuilt
// libraries from GitHub into each Cargo target folder. A network that
// inspects TLS refuses that download, so a fresh native target folder could
// not build. A previous build keeps the libraries under
// target/sherpa-onnx-prebuilt; later builds (and other git worktrees) reuse
// them through SHERPA_ONNX_LIB_DIR, and download them only when none are there.
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

const ARCHIVES = {
  "win32-x64": "win-x64-static-MT-Release-lib",
  "win32-arm64": "win-arm64-static-MT-Release-lib",
  "darwin-arm64": "osx-arm64-static-lib",
  "darwin-x64": "osx-x64-static-lib",
  "linux-x64": "linux-x64-static-lib",
  "linux-arm64": "linux-aarch64-static-lib",
};

/** The folder sherpa-onnx-sys unpacks for this platform's static build. */
export function sherpaArchive(version, platform = process.platform, arch = process.arch) {
  const name = ARCHIVES[`${platform}-${arch}`];
  return name ? `sherpa-onnx-v${version}-${name}` : undefined;
}

/** The sherpa-onnx-sys version the workspace lockfile pins. */
export function sherpaVersion(root) {
  let lock;
  try {
    lock = readFileSync(join(root, "Cargo.lock"), "utf8");
  } catch {
    return undefined;
  }
  return /name = "sherpa-onnx-sys"\r?\nversion = "([^"]+)"/.exec(lock)?.[1];
}

/** This checkout and, from a git worktree, the main one. */
function checkouts(root) {
  const roots = [root];
  const common = spawnSync("git", ["rev-parse", "--path-format=absolute", "--git-common-dir"], {
    cwd: root,
    encoding: "utf8",
  });
  if (common.status === 0 && common.stdout.trim()) roots.push(dirname(common.stdout.trim()));
  return [...new Set(roots.map((path) => resolve(path)))];
}

/** `env` with SHERPA_ONNX_LIB_DIR pointing at libraries already on disk. */
export function sherpaEnv(root, env = process.env, options = {}) {
  if (env.SHERPA_ONNX_LIB_DIR) return env;
  const version = sherpaVersion(root);
  const archive = version && sherpaArchive(version, options.platform, options.arch);
  if (!archive) return env;
  for (const checkout of options.roots ?? checkouts(root)) {
    const lib = join(checkout, "target", "sherpa-onnx-prebuilt", archive, "lib");
    if (existsSync(lib)) return { ...env, SHERPA_ONNX_LIB_DIR: lib };
  }
  return env;
}
