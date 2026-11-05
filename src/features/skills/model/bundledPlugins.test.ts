import { describe, expect, it } from "vitest";
import { buildClaudeSpawnArgs } from "../../../integrations/harness/providers/claude/claudeProtocol";
import { bundledPluginFiles } from "./bundledPlugins";

const frontmatterName = (content: string) =>
  content.match(/^---\r?\nname: ([a-z0-9-]+)\r?\n/)?.[1];

describe("bundled plugins", () => {
  it("ships the monobot plugin with its manifest and skills", async () => {
    const files = await bundledPluginFiles("monobot");
    const listed = files.map((file) => file.path);
    expect(listed).toContain(".claude-plugin/plugin.json");
    const manifest = files.find(
      (file) => file.path === ".claude-plugin/plugin.json",
    )!;
    expect(JSON.parse(manifest.content).name).toBe("monobot");

    const skills = files.filter((file) =>
      /^skills\/[^/]+\/SKILL\.md$/.test(file.path),
    );
    expect(skills.map((file) => frontmatterName(file.content)).sort()).toEqual([
      "animate",
      "animate-expo",
      "animation-performance",
      "animation-vocabulary",
      "apple-design",
      "ask-sonner",
      "break-ui",
      "choreograph",
      "emil-design-eng",
      "files",
      "find-animation-opportunities",
      "improve-animations",
      "mobile-native",
      "pick-ui-library",
      "prototype",
      "remove-animations",
      "research",
      "review-animations",
      "threejs-graphics",
      "write-swift",
    ]);
    // Emil Kowalski's design skills ship with their license, and every file
    // they link to ships beside them.
    const paths = new Set(files.map((file) => file.path));
    for (const skill of skills.filter((file) =>
      paths.has(file.path.replace("SKILL.md", "NOTICE.md")),
    )) {
      const folder = skill.path.replace("SKILL.md", "");
      for (const match of skill.content.matchAll(/\]\(([A-Z-]+\.md)\)/g))
        expect(paths, `${folder}${match[1]}`).toContain(`${folder}${match[1]}`);
    }
    expect(paths).toContain("skills/emil-design-eng/NOTICE.md");
    expect(
      listed.filter((path) =>
        path.startsWith("skills/threejs-graphics/references/"),
      ),
    ).toHaveLength(72);
    expect(listed).toContain("skills/threejs-graphics/NOTICE.md");
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
