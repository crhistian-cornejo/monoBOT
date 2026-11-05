import { parseWorkbook } from "./spreadsheet";

/** Reads a workbook off the main thread: large sheets take a while to unzip. */
self.onmessage = async ({
  data,
}: MessageEvent<
  | { warm: true }
  | { id: number; bytes: Uint8Array; path: string; locale?: string }
>) => {
  // Load the readers ahead of the first file, so it opens without waiting.
  if ("warm" in data) {
    void Promise.all([import("exceljs"), import("jszip")]).catch(() => {});
    return;
  }
  try {
    self.postMessage({
      id: data.id,
      result: await parseWorkbook(data.bytes, data.path, data.locale),
    });
  } catch (error) {
    self.postMessage({
      id: data.id,
      error: error instanceof Error ? error.message : String(error),
    });
  }
};
