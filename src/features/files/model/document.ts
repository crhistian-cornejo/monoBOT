/** A document the agents' panel can show as the file draws it. */
export type DocumentKind = "pdf" | "docx" | "pptx";

const KINDS: Record<string, DocumentKind> = {
  pdf: "pdf",
  docx: "docx",
  docm: "docx",
  pptx: "pptx",
  pptm: "pptx",
  ppsx: "pptx",
};

function extensionOf(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

/**
 * The kind of document at a path, or undefined. The old binary formats
 * (.doc, .ppt) are not read here; they open in the system's app.
 */
export function documentKind(path: string): DocumentKind | undefined {
  return KINDS[extensionOf(path)];
}

export function isDocumentPath(path: string): boolean {
  return !!documentKind(path);
}
