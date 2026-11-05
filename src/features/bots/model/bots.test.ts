import { describe, expect, it } from "vitest";
import { buildClaudeSpawnArgs } from "../../../integrations/harness/providers/claude/claudeProtocol";
import { BOT_ROLES, botLaunch, isBotRole, parseBotTools } from "./bots";

describe("bots", () => {
  it("knows its roles", () => {
    expect(BOT_ROLES.map((info) => info.role)).toEqual([
      "principal",
      "assistant",
      "finance",
      "documents",
      "research",
      "custom",
    ]);
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
    expect(
      launch.allowedTools.every((tool) =>
        tool.startsWith("mcp__claude_ai_Notion__"),
      ),
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

  it("passes the denied tools to Claude", () => {
    const { disallowedTools } = botLaunch("assistant");
    const args = buildClaudeSpawnArgs({ disallowedTools });
    expect(args[args.indexOf("--disallowedTools") + 1]).toBe(
      disallowedTools.join(","),
    );
  });
});
