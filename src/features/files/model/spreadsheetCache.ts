import { readBinaryFile, statFiles } from "../../../platform/tauri/fs";
import type { Workbook } from "./spreadsheet";
import { readSpreadsheet } from "./spreadsheetWorker";

/**
 * Workbooks already read, by path and modification time: going back to a
 * sheet, a second mount, or two panes on one file read it once. A file that
 * changes on disk has a new time, so it is read again.
 */

/** A few: each holds its pictures' bytes. */
export const MAX_CACHED = 4;

export type SpreadsheetIo = {
  stat: (
    paths: string[],
  ) => Promise<{ path: string; mtimeMs: number | null }[]>;
  read: (path: string) => Promise<Uint8Array>;
  parse: (
    bytes: Uint8Array,
    path: string,
    signal: AbortSignal,
  ) => Promise<Workbook>;
};

const appIo: SpreadsheetIo = {
  stat: statFiles,
  read: readBinaryFile,
  parse: readSpreadsheet,
};

const cache = new Map<string, Promise<Workbook>>();
/** A shared read is never cancelled: a later opener may still want it. */
const shared = new AbortController().signal;

export function clearSpreadsheetCache() {
  cache.clear();
}

function whenNotAborted<T>(job: Promise<T>, signal: AbortSignal): Promise<T> {
  if (signal.aborted)
    return Promise.reject(
      new DOMException("Spreadsheet read cancelled", "AbortError"),
    );
  return new Promise<T>((resolve, reject) => {
    const abort = () =>
      reject(new DOMException("Spreadsheet read cancelled", "AbortError"));
    signal.addEventListener("abort", abort, { once: true });
    job.then(
      (value) => {
        signal.removeEventListener("abort", abort);
        resolve(value);
      },
      (error: unknown) => {
        signal.removeEventListener("abort", abort);
        reject(error);
      },
    );
  });
}

/** Read and parse a spreadsheet, or hand back the copy already read. */
export async function openSpreadsheet(
  path: string,
  signal: AbortSignal,
  io: SpreadsheetIo = appIo,
): Promise<Workbook> {
  // A file that cannot be stated is read every time, never cached.
  const [info] = await Promise.resolve()
    .then(() => io.stat([path]))
    .catch(() => []);
  const key = info?.mtimeMs != null ? `${path}\n${info.mtimeMs}` : null;
  const hit = key ? cache.get(key) : undefined;
  if (key && hit) {
    // Most recently used last.
    cache.delete(key);
    cache.set(key, hit);
    return whenNotAborted(hit, signal);
  }
  const job = io.read(path).then((bytes) => io.parse(bytes, path, shared));
  if (key) {
    for (const old of [...cache.keys()])
      if (old.startsWith(`${path}\n`)) cache.delete(old);
    cache.set(key, job);
    job.catch(() => cache.delete(key));
    while (cache.size > MAX_CACHED) cache.delete(cache.keys().next().value!);
  }
  return whenNotAborted(job, signal);
}
