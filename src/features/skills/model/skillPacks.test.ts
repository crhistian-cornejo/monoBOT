// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import { bundledPluginFiles } from "./bundledPlugins";
import {
  enabledSkillPacks,
  isSkillPackEnabled,
  setSkillPackEnabled,
  SKILL_PACKS,
} from "./skillPacks";

const frontmatterName = (content: string) =>
  content.match(/^---\r?\nname: "?([a-z0-9-]+)"?\r?\n/)?.[1];

beforeEach(() => localStorage.clear());

describe("skill packs", () => {
  it("stays off until the user turns a pack on", () => {
    expect(enabledSkillPacks()).toEqual([]);
    setSkillPackEnabled("monobot-design", true);
    expect(isSkillPackEnabled("monobot-design")).toBe(true);
    expect(enabledSkillPacks()).toEqual(["monobot-design"]);
    setSkillPackEnabled("monobot-design", false);
    expect(enabledSkillPacks()).toEqual([]);
  });

  it.each(SKILL_PACKS.map((pack) => [pack.id, pack] as const))(
    "%s ships every skill it lists, with its license, under the size limit",
    async (_id, pack) => {
      const files = await bundledPluginFiles(pack.id);
      const manifest = files.find(
        (file) => file.path === ".claude-plugin/plugin.json",
      );
      expect(JSON.parse(manifest!.content).name).toBe(pack.id);
      const skills = files.filter((file) =>
        /^skills\/[^/]+\/SKILL\.md$/.test(file.path),
      );
      expect(skills.map((file) => file.path.split("/")[1]).sort()).toEqual(
        [...pack.skills].sort(),
      );
      for (const skill of skills)
        expect(frontmatterName(skill.content), skill.path).toBeTruthy();
      const paths = new Set(files.map((file) => file.path));
      for (const name of pack.skills) {
        const notice = files.find(
          (file) => file.path === `skills/${name}/NOTICE.md`,
        );
        expect(notice?.content, name).toMatch(/License/);
      }
      // No platform binaries or images: everything is text.
      expect(
        [...paths].some((path) => /\/bin\/|\.(png|svg|zip)$/.test(path)),
      ).toBe(false);
      const bytes = files.reduce((sum, file) => sum + file.content.length, 0);
      expect(bytes).toBeLessThan(8 * 1024 * 1024);
    },
  );
});
