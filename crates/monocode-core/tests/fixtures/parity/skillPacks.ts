import { readFlag, writeFlag } from "../../settings/model/storageFlags";
import type { SkillPackId } from "./bundledPlugins";

/**
 * Third-party skills MonoBOT ships, gathered from the user's Claude Code and
 * Codex setups, in packs that stay off until turned on. Claude loads an
 * enabled pack as a plugin; the other agents get a skill's instructions when
 * the user calls it by `/name`. Each skill folder keeps its source and
 * license in NOTICE.md.
 */
export type SkillPack = {
  id: SkillPackId;
  name: string;
  description: string;
  skills: readonly string[];
  /** A key the skills' scripts read from the environment, when they need one. */
  needs?: string;
};

export const SKILL_PACKS: readonly SkillPack[] = [
  {
    id: "monobot-design",
    name: "Design and taste",
    description:
      "Impeccable, the taste skills, motion principles and Vercel's React and web interface guidelines.",
    skills: [
      "impeccable",
      "design-taste-frontend",
      "gpt-taste",
      "high-end-visual-design",
      "minimalist-ui",
      "industrial-brutalist-ui",
      "stitch-design-taste",
      "design-motion-principles",
      "vercel-react-best-practices",
      "web-design-guidelines",
    ],
  },
  {
    id: "monobot-imagery",
    name: "Image direction",
    description:
      "Design comps for web and mobile, image-to-code and brand kits, generated before building.",
    skills: [
      "imagegen-frontend-web",
      "imagegen-frontend-mobile",
      "image-to-code",
      "brandkit",
    ],
  },
  {
    id: "monobot-dev",
    name: "Developer",
    description:
      "AI SDK, Context7, Postgres and Python performance, GitHub review comments and CI, PDFs and Vercel deploys.",
    skills: [
      "ai-sdk",
      "context7",
      "supabase-postgres-best-practices",
      "python-performance-optimization",
      "find-skills",
      "gh-address-comments",
      "gh-fix-ci",
      "pdf",
      "vercel-deploy",
    ],
  },
  {
    id: "monobot-audio",
    name: "ElevenLabs audio",
    description:
      "Text to speech, transcription, voices, dubbing, sound effects and music.",
    skills: [
      "text-to-speech",
      "speech-to-text",
      "sound-effects",
      "music",
      "dubbing",
      "voice-changer",
      "voice-isolator",
      "agents",
      "speech-engine",
      "setup-api-key",
    ],
    needs: "ELEVENLABS_API_KEY",
  },
  {
    id: "monobot-openai",
    name: "OpenAI media",
    description: "Images and Sora video with OpenAI's API.",
    skills: ["imagegen", "sora"],
    needs: "OPENAI_API_KEY",
  },
];

export const SKILL_PACKS_CHANGED = "monocode:skill-packs-changed";

const flagKey = (id: SkillPackId) => `monocode.skillPack.${id}.enabled`;

export function isSkillPackEnabled(id: SkillPackId): boolean {
  return readFlag(flagKey(id)) ?? false;
}

export function setSkillPackEnabled(id: SkillPackId, enabled: boolean): void {
  writeFlag(flagKey(id), enabled);
  if (typeof window !== "undefined")
    window.dispatchEvent(new CustomEvent(SKILL_PACKS_CHANGED));
}

export function enabledSkillPacks(): SkillPackId[] {
  return SKILL_PACKS.filter((pack) => isSkillPackEnabled(pack.id)).map(
    (pack) => pack.id,
  );
}

export function subscribeSkillPacks(listener: () => void): () => void {
  if (typeof window === "undefined") return () => {};
  window.addEventListener(SKILL_PACKS_CHANGED, listener);
  window.addEventListener("storage", listener);
  return () => {
    window.removeEventListener(SKILL_PACKS_CHANGED, listener);
    window.removeEventListener("storage", listener);
  };
}

/** A stable snapshot for `useSyncExternalStore`. */
let snapshot = "";
let snapshotList: SkillPackId[] = [];
export function enabledSkillPacksSnapshot(): SkillPackId[] {
  const next = enabledSkillPacks();
  const key = next.join(",");
  if (key !== snapshot) {
    snapshot = key;
    snapshotList = next;
  }
  return snapshotList;
}
