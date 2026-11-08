#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
  console.error("usage: pnpm set-version -- 0.1.1");
  process.exit(1);
}

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function replaceFirst(path, pattern, replacement) {
  const text = readFileSync(path, "utf8");
  const next = text.replace(pattern, replacement);
  if (next === text) {
    console.error(`failed to update ${path}`);
    process.exit(1);
  }
  writeFileSync(path, next);
}

replaceFirst(
  join(root, "package.json"),
  /("version": ")[^"]+(")/,
  `$1${version}$2`,
);
// The remote host reports this version, and the app only accepts a host whose
// version matches its own CARGO_PKG_VERSION (crates/monocode-core/src/remote).
replaceFirst(
  join(root, "host/package.json"),
  /("version": ")[^"]+(")/,
  `$1${version}$2`,
);
replaceFirst(
  join(root, "Cargo.toml"),
  /^(version = ")[^"]+(")/m,
  `$1${version}$2`,
);
for (const crate of ["monocode-core", "monocode-gpui"]) {
  replaceFirst(
    join(root, "Cargo.lock"),
    new RegExp(`(name = "${crate}"\nversion = ")[^"]+(")`),
    `$1${version}$2`,
  );
}

console.log(`version ${version}`);
