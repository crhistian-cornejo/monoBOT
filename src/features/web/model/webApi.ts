import { invoke } from "@tauri-apps/api/core";

/** A page as the app fetched it (src-tauri/src/web.rs). */
export type FetchedWebPage = {
  finalUrl: string;
  contentType?: string;
  /** HTML or text; absent for a PDF or another binary file. */
  text?: string;
  isPdf: boolean;
  bytes: number;
};

export type DownloadedFile = {
  path: string;
  finalUrl: string;
  contentType?: string;
  bytes: number;
  isPdf: boolean;
};

export function fetchWebPage(url: string): Promise<FetchedWebPage> {
  return invoke("web_page", { url });
}

/** A PDF lands in the app's cache to be read as a paper; other files in Documents. */
export function downloadWebFile(
  url: string,
  options: { agent?: string; name?: string } = {},
): Promise<DownloadedFile> {
  return invoke("web_download", {
    url,
    agent: options.agent ?? "",
    name: options.name ?? null,
  });
}

/** Copies a downloaded PDF to Documents/MonoBOT/<agent>, for one not read as a paper. */
export function keepDownload(path: string, agent?: string): Promise<string> {
  return invoke("web_keep", { path, agent: agent ?? "" });
}
