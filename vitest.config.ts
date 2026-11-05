import { defineConfig } from "vitest/config";

// Node 25 turns its own Web Storage on by default. Without a backing file its
// `localStorage` is an empty stub that shadows the one happy-dom installs, so
// every test that touches storage fails. Turn it off (the flag exists since 22).
const nodeMajor = Number(process.versions.node.split(".")[0]);
const execArgv = nodeMajor >= 22 ? ["--no-experimental-webstorage"] : [];

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
    poolOptions: {
      forks: { execArgv },
      threads: { execArgv },
    },
  },
});
