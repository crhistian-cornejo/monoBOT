import { describe, expect, it } from "vitest";
import { buildClaudeSpawnArgs } from "../../../integrations/harness/providers/claude/claudeProtocol";
import { bundledPluginFiles } from "./bundledPlugins";

const frontmatterName = (content: string) =>
  content.match(/^---\r?\nname: ([a-z0-9-]+)\r?\n/)?.[1];

describe("bundled plugins", () => {
  it("ships the monobot plugin with its manifest and skills", async () => {
    const files = await bundledPluginFiles("monobot");
    const paths = files.map((file) => file.path);
    expect(paths).toContain(".claude-plugin/plugin.json");
    const manifest = files.find(
      (file) => file.path === ".claude-plugin/plugin.json",
    )!;
    expect(JSON.parse(manifest.content).name).toBe("monobot");

    const skills = files.filter((file) =>
      /^skills\/[^/]+\/SKILL\.md$/.test(file.path),
    );
    expect(skills.map((file) => frontmatterName(file.content)).sort()).toEqual([
      "files",
      "research",
      "threejs-graphics",
    ]);
    expect(
      paths.filter((path) =>
        path.startsWith("skills/threejs-graphics/references/"),
      ),
    ).toHaveLength(72);
    expect(paths).toContain("skills/threejs-graphics/NOTICE.md");
  });

  it("keeps the three.js guides pointing at local references", async () => {
    const files = await bundledPluginFiles("monobot");
    for (const file of files) {
      expect(file.content, file.path).not.toContain("use_skill");
    }
    const router = files.find(
      (file) => file.path === "skills/threejs-graphics/SKILL.md",
    )!;
    const referenced = [
      ...router.content.matchAll(/references\/([a-z0-9-]+)\.md/g),
    ].map((match) => `skills/threejs-graphics/references/${match[1]}.md`);
    const paths = new Set(files.map((file) => file.path));
    expect(referenced.length).toBeGreaterThan(0);
    for (const path of referenced) expect(paths, path).toContain(path);
  });

  it("keeps the finance skill in its own plugin", async () => {
    const files = await bundledPluginFiles("monobot-finance");
    expect(files.map((file) => file.path).sort()).toEqual([
      ".claude-plugin/plugin.json",
      "skills/finance/SKILL.md",
    ]);
  });

  it("loads plugin folders for interactive turns only", () => {
    const args = buildClaudeSpawnArgs({ pluginDirs: ["/data/plugins/monobot"] });
    expect(args[args.indexOf("--plugin-dir") + 1]).toBe(
      "/data/plugins/monobot",
    );
    expect(
      buildClaudeSpawnArgs({ isolated: true, pluginDirs: ["/x"] }),
    ).not.toContain("--plugin-dir");
  });
});
