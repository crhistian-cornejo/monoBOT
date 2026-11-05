import { describe, expect, it } from "vitest";
import { modelsFromSessionNew } from "../antigravity/antigravityProtocol";
import {
  DROID_ACP_ARGS,
  DROID_AUTH_HELP,
  droidError,
  droidModeId,
} from "./droidProtocol";

// Trimmed from a real `session/new` reply of droid 0.251.0.
const SESSION_NEW = {
  sessionId: "6f4d9429-05ee-4f2a-8f41-2184e9c698eb",
  models: {
    currentModelId: "gpt-6.1-sol",
    availableModels: [
      { modelId: "auto", name: "Auto Model" },
      { modelId: "claude-opus-5-5", name: "Opus 5.5" },
    ],
  },
  modes: { currentModeId: "auto-medium" },
  configOptions: [
    {
      id: "autonomy_level",
      category: "mode",
      type: "select",
      currentValue: "auto-medium",
      options: [{ value: "normal", name: "Auto (Off)" }],
    },
    {
      id: "model",
      category: "model",
      type: "select",
      currentValue: "gpt-6.1-sol",
      options: [
        { value: "auto", name: "Auto Model" },
        { value: "claude-opus-5-5", name: "Opus 5.5" },
        { value: "gpt-6.1-sol", name: "GPT-6.1 Sol" },
      ],
    },
    {
      id: "reasoning_effort",
      category: "thought_level",
      type: "select",
      currentValue: "high",
      options: [
        { value: "low", name: "Low" },
        { value: "medium", name: "Medium" },
        { value: "high", name: "High" },
        { value: "max", name: "Max" },
      ],
    },
  ],
};

describe("droid", () => {
  it("starts its ACP server through exec", () => {
    expect([...DROID_ACP_ARGS]).toEqual(["exec", "-o", "acp"]);
  });

  it("maps MonoBOT's permission modes onto Droid's autonomy", () => {
    expect(droidModeId("supervised")).toBe("normal");
    expect(droidModeId("auto-accept-edits")).toBe("auto-low");
    expect(droidModeId("auto")).toBe("auto-medium");
    expect(droidModeId("full-access")).toBe("auto-high");
    // Planning is Droid's read-only Spec mode, whatever the permissions.
    expect(droidModeId("full-access", true)).toBe("spec");
  });

  it("lists its models with their effort levels", () => {
    const models = modelsFromSessionNew(SESSION_NEW, "droid");
    expect(models.map((model) => model.id)).toEqual([
      "droid:auto",
      "droid:claude-opus-5-5",
      "droid:gpt-6.1-sol",
    ]);
    expect(models.every((model) => model.harness === "droid")).toBe(true);
    expect(models[1]).toMatchObject({ name: "Opus 5.5", nativeId: "claude-opus-5-5" });
    expect(models[0]?.settings?.[0]).toMatchObject({
      id: "effort",
      value: "high",
    });
    expect(
      models[0]?.settings?.[0]?.options?.map((option) => option.value),
    ).toEqual(["low", "medium", "high", "max"]);
  });

  it("points a sign-in failure at Droid's own login, once", () => {
    const error = droidError(new Error("Unauthorized: run login"));
    expect(error.message).toContain(DROID_AUTH_HELP);
    expect(droidError(error).message.split(DROID_AUTH_HELP)).toHaveLength(2);
    expect(droidError(new Error("Disk full")).message).toBe("Disk full");
  });
});
