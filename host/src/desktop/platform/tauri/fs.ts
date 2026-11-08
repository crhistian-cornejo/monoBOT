// Desktop file and Git commands that provider adapters still name. On the
// host each one rejects (see ../host/invoke), which the adapters already
// treat as "not available here"; the host's own file and Git endpoints are in
// host/workspace.ts and host/workspace-commands.ts. The types are the shapes
// the desktop reads from those endpoints.
import { invoke } from "../host/invoke";

export type FsEntry = {
  name: string;
  path: string;
  isDir: boolean;
  ignored: boolean;
};

export type ProjectFile = {
  name: string;
  path: string;
  relative: string;
  isDir?: boolean;
};

export type GitChangedFile = {
  path: string;
  relative: string;
  status: "modified" | "added" | "deleted" | "untracked" | string;
  additions: number;
  deletions: number;
  staged: boolean;
  unstaged: boolean;
};

export type GitDiffIndex = {
  branch: string | null;
  head: string | null;
  files: GitChangedFile[];
  additions: number;
  deletions: number;
  remote: string | null;
  upstream: string | null;
  defaultBranch: string | null;
  ahead: number;
  behind: number;
  aheadOfDefault: number;
  headPushed: boolean;
};

export type GitFileDiff = {
  path: string;
  relative: string;
  status: string;
  original: string;
  current: string;
  binary: boolean;
  tooLarge: boolean;
};

export type GitStagedContext = {
  branch: string | null;
  summary: string;
  patch: string;
};

export function gitStagedContext(
  cwd: string,
  paths?: readonly string[],
): Promise<GitStagedContext> {
  return invoke<GitStagedContext>("git_staged_context", {
    cwd,
    ...(paths === undefined ? {} : { paths }),
  });
}

export type GitRangeContext = {
  base: string;
  head: string;
  commitSummary: string;
  diffSummary: string;
  diffPatch: string;
};

export function gitRangeContext(cwd: string): Promise<GitRangeContext> {
  return invoke<GitRangeContext>("git_range_context", { cwd });
}

export type GitPr = {
  number: number;
  title: string;
  url: string;
  state: string;
};

/** Headless hosts have no desktop home lookup; callers fall back without it. */
export function homeDir(): Promise<string> {
  return invoke<string>("home_dir");
}

export type FileMtime = {
  path: string;
  mtimeMs: number | null;
};

export type GeneratedImageAsset = {
  path: string;
  mimeType: string;
  size: number;
};

export function saveGeneratedImage(input: {
  data: string;
  name: string;
}): Promise<GeneratedImageAsset> {
  return invoke<GeneratedImageAsset>("save_generated_image", input);
}

/** Pictures Antigravity's Gemini wrote since `sinceMs`, copied into the app's folder. */
export function collectAntigravityImages(
  sinceMs: number,
): Promise<{ path: string; name: string; mimeType: string; size: number }[]> {
  return invoke("collect_antigravity_images", {
    sinceMs: Math.floor(sinceMs),
  });
}

export function deleteGeneratedImages(paths: string[]): Promise<void> {
  return invoke<void>("delete_generated_images", { paths });
}
