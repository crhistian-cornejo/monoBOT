import { describe, expect, it } from "vitest";
import { buildClaudeSpawnArgs } from "../../../integrations/harness/providers/claude/claudeProtocol";
import {
  BOT_ROLES,
  botLaunch,
  botTools,
  deniedSkills,
  folderProblem,
  isBotRole,
  parseBotTools,
  rulePath,
  skillMatches,
} from "./bots";

describe("bots", () => {
  it("gives the image bot Nano Banana through its Images access", () => {
    expect(botTools("images").access).toContain("images");
    expect(botTools("research").access).not.toContain("images");
  });

  it("knows its roles", () => {
    expect(BOT_ROLES.map((info) => info.role)).toEqual([
      "principal",
      "assistant",
      "inbox",
      "planner",
      "writer",
      "analyst",
      "finance",
      "documents",
      "research",
      "images",
      "custom",
    ]);
    // Every role says what it takes on, for the orchestrator to pick by.
    expect(BOT_ROLES.every((info) => info.specialty.trim().length > 0)).toBe(
      true,
    );
    // The Principal has its own row; a custom bot comes from "Blank bot".
    expect(
      BOT_ROLES.filter((info) => info.hidden).map((info) => info.role),
    ).toEqual(["principal", "custom"]);
    expect(isBotRole("finance")).toBe(true);
    expect(isBotRole("coder")).toBe(false);
  });

  it("gives the finance bot its skill, Notion and read-only Gmail", () => {
    const launch = botLaunch("finance");
    expect(launch.plugins).toEqual(["monobot-finance"]);
    expect(launch.allowedTools).toContain(
      "mcp__claude_ai_Gmail__search_threads",
    );
    expect(launch.allowedTools).toContain(
      "mcp__claude_ai_Notion__create-pages",
    );
    expect(launch.allowedTools).toContain(
      "mcp__claude_ai_Notion__notion-create-pages",
    );
    expect(launch.disallowedTools).toContain(
      "mcp__claude_ai_Gmail__send_message",
    );
    // Drive is not part of its role, so the whole server stays out.
    expect(launch.disallowedTools).toContain("mcp__claude_ai_Google_Drive");
    expect(
      launch.allowedTools.some((tool) =>
        tool.startsWith("mcp__claude_ai_Google_Drive"),
      ),
    ).toBe(false);
  });

  it("never pre-allows a tool it also denies", () => {
    for (const { role } of BOT_ROLES) {
      const { allowedTools, disallowedTools } = botLaunch(role);
      const denied = new Set(disallowedTools);
      expect(
        allowedTools.filter((tool) => denied.has(tool)),
        role,
      ).toEqual([]);
      for (const send of [
        "send_message",
        "reply",
        "forward",
        "trash_message",
      ]) {
        expect(allowedTools, role).not.toContain(
          `mcp__claude_ai_Gmail__${send}`,
        );
      }
    }
  });

  it("keeps every bot from deleting files or pushing", () => {
    for (const { role } of BOT_ROLES) {
      const { disallowedTools } = botLaunch(role);
      expect(disallowedTools, role).toContain("Bash(rm:*)");
      expect(disallowedTools, role).toContain("Bash(Remove-Item:*)");
      expect(disallowedTools, role).toContain("Bash(git push:*)");
    }
  });

  it("uses a bot's own tools, still denying what sends or deletes", () => {
    const launch = botLaunch("assistant", {
      services: ["notion"],
      plugins: ["monobot-finance"],
    });
    expect(launch.plugins).toEqual(["monobot-finance"]);
    expect(launch.disallowedTools).toContain("mcp__claude_ai_Gmail");
    expect(launch.disallowedTools).toContain("mcp__claude_ai_Google_Drive");
    expect(launch.disallowedTools).toContain(
      "mcp__claude_ai_Notion__delete-pages",
    );
    expect(launch.disallowedTools).toContain("Bash(rm:*)");
    // Of the user's accounts, only Notion's tools are pre-allowed.
    expect(
      launch.allowedTools
        .filter((tool) => tool.startsWith("mcp__claude_ai_"))
        .every((tool) => tool.startsWith("mcp__claude_ai_Notion__")),
    ).toBe(true);
  });

  it("keeps only the services and skills it knows", () => {
    expect(
      parseBotTools({
        services: ["gmail", "slack"],
        plugins: ["evil", "monobot-finance"],
      }),
    ).toEqual({ services: ["gmail"], plugins: ["monobot-finance"] });
    expect(parseBotTools("nope")).toBeUndefined();
  });

  it("leaves subagents to the bots, not the Principal", () => {
    expect(botLaunch("principal").disallowedTools).toEqual(
      expect.arrayContaining(["Task", "Agent"]),
    );
    expect(botLaunch("research").disallowedTools).not.toContain("Task");
  });

  it("writes folder rules the way Claude compares paths", () => {
    expect(rulePath(String.raw`C:\Users\Ana\Docs\ `.trim())).toBe(
      "c/Users/Ana/Docs",
    );
    expect(rulePath("D:/Facturas")).toBe("d/Facturas");
    expect(rulePath("/Users/ana/Docs/")).toBe("Users/ana/Docs");
  });

  it("opens any folder to read but never a disk or a system folder to change", () => {
    expect(folderProblem("C:\\", false)).toMatch(/whole disk/);
    expect(folderProblem("Docs", false)).toMatch(/full path/);
    const system = String.raw`C:\Windows\System32`;
    expect(folderProblem(system, false)).toBeUndefined();
    expect(folderProblem(system, true)).toMatch(/never changed/);
    expect(folderProblem("/etc/hosts", true)).toMatch(/never changed/);
    expect(folderProblem("/Users/ana/Docs", true)).toBeUndefined();
  });

  it("gives a bot its folders and access, and denies what it lacks", () => {
    const launch = botLaunch("custom", {
      services: [],
      plugins: [],
      access: ["read", "edit"],
      folders: [
        { path: String.raw`D:\Facturas`, edit: true },
        { path: String.raw`C:\Windows`, edit: false },
      ],
      connectors: ["context7"],
    });
    expect(launch.addDirs).toEqual([
      String.raw`D:\Facturas`,
      String.raw`C:\Windows`,
    ]);
    expect(launch.allowedTools).toEqual(
      expect.arrayContaining([
        "Read(//d/Facturas/**)",
        "Edit(//d/Facturas/**)",
        "Read(//c/Windows/**)",
      ]),
    );
    expect(launch.allowedTools).not.toContain("Edit(//c/Windows/**)");
    expect(launch.disallowedTools).toEqual(
      expect.arrayContaining(["WebSearch", "WebFetch"]),
    );
    expect(launch.connectors).toEqual(["context7"]);
    // Every bot can search the web and read until told otherwise.
    expect(botLaunch("assistant").allowedTools).toEqual(
      expect.arrayContaining(["WebSearch", "Read"]),
    );
  });

  it("passes the denied tools to Claude", () => {
    const { disallowedTools } = botLaunch("assistant");
    const args = buildClaudeSpawnArgs({ disallowedTools });
    expect(args[args.indexOf("--disallowedTools") + 1]).toBe(
      disallowedTools.join(","),
    );
  });

  it("gives each role the connectors and skills of its job, not all of them", () => {
    // The orchestrator does its own everyday job with the user's accounts,
    // without the specialists' connectors and skills.
    expect(botTools("principal")).toMatchObject({
      services: ["notion", "gmail", "drive"],
      connectors: [],
      skills: [],
    });
    expect(botTools("research").connectors).toEqual([
      "context7",
      "deepwiki",
      "mslearn",
      "xmd",
      "openalex",
    ]);
    expect(botTools("documents").skills).toEqual(["docx", "xlsx", "pptx", "pdf"]);
    expect(botTools("inbox")).toMatchObject({ services: ["gmail"], access: ["read"] });
    // A custom bot keeps every connector and skill until the user picks.
    expect(botTools("custom").connectors).toBeUndefined();
    expect(botTools("custom").skills).toBeUndefined();
    // Its own list wins over the role's.
    expect(botTools("documents", { services: [], plugins: [], skills: ["pdf"] }).skills).toEqual(["pdf"]);
  });

  it("denies the installed skills a bot was not given", () => {
    const installed = ["pdf", "document-skills:xlsx", "deploy", "anthropic-skills:docx"];
    // A role's hint covers the plugin that ships the skill.
    expect(skillMatches("xlsx", "document-skills:xlsx")).toBe(true);
    expect(skillMatches("xlsx", "xlsx-helper")).toBe(false);
    expect(deniedSkills(["docx", "xlsx", "pdf"], installed)).toEqual(["deploy"]);
    expect(deniedSkills(undefined, installed)).toEqual([]);
    expect(botLaunch("documents", undefined, installed).disallowedTools).toEqual(
      expect.arrayContaining(["Skill(deploy)"]),
    );
    expect(botLaunch("documents", undefined, installed).disallowedTools).not.toContain(
      "Skill(pdf)",
    );
    // The orchestrator keeps none of them; a custom bot keeps all.
    expect(
      botLaunch("principal", undefined, installed).disallowedTools.filter((tool) =>
        tool.startsWith("Skill("),
      ),
    ).toHaveLength(4);
    expect(
      botLaunch("custom", undefined, installed).disallowedTools.some((tool) =>
        tool.startsWith("Skill("),
      ),
    ).toBe(false);
  });

  it("keeps only skill names Claude could invoke", () => {
    expect(
      parseBotTools({
        services: [],
        plugins: [],
        skills: ["pdf", "pdf", "a b", "plugin:xlsx", 4],
      }),
    ).toEqual({ services: [], plugins: [], skills: ["pdf", "plugin:xlsx"] });
  });
});
