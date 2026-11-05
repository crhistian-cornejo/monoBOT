import { parseWorkbook, type Workbook } from "./spreadsheet";

type Reply = { id: number; result?: Workbook; error?: string };

let worker: Worker | null = null;
let broken = false;
let sequence = 0;
const pending = new Map<
  number,
  { resolve: (workbook: Workbook) => void; reject: (error: Error) => void }
>();

function failAll(error: Error) {
  for (const job of pending.values()) job.reject(error);
  pending.clear();
}

function spawn(): Worker | null {
  if (worker || broken) return worker;
  try {
    worker = new Worker(new URL("./spreadsheet.worker.ts", import.meta.url), {
      type: "module",
    });
  } catch {
    broken = true;
    return null;
  }
  worker.onmessage = ({ data }: MessageEvent<Reply>) => {
    const job = pending.get(data.id);
    if (!job) return;
    pending.delete(data.id);
    if (data.error !== undefined) job.reject(new Error(data.error));
    else if (data.result) job.resolve(data.result);
  };
  worker.onerror = () => {
    // A worker that cannot start is not retried: the main thread reads instead.
    worker?.terminate();
    worker = null;
    broken = true;
    failAll(new Error("worker-failed"));
  };
  return worker;
}

/**
 * Read a spreadsheet for the viewer, in a worker when one starts. Cancelling
 * drops the answer; the worker finishes the file it was on.
 */
export function readSpreadsheet(
  bytes: Uint8Array,
  path: string,
  signal: AbortSignal,
): Promise<Workbook> {
  const locale =
    typeof navigator === "undefined" ? undefined : navigator.language;
  const target = spawn();
  if (!target) return parseWorkbook(bytes, path, locale);
  const id = ++sequence;
  return new Promise<Workbook>((resolve, reject) => {
    const abort = () => {
      pending.delete(id);
      reject(new DOMException("Spreadsheet read cancelled", "AbortError"));
    };
    if (signal.aborted) return abort();
    signal.addEventListener("abort", abort, { once: true });
    pending.set(id, {
      resolve: (workbook) => {
        signal.removeEventListener("abort", abort);
        resolve(workbook);
      },
      reject: (error) => {
        signal.removeEventListener("abort", abort);
        if (error.message === "worker-failed")
          parseWorkbook(bytes, path, locale).then(resolve, reject);
        else reject(error);
      },
    });
    // The bytes are copied, not transferred: the main thread may need them
    // again if the worker fails.
    target.postMessage({ id, bytes, path, locale });
  });
}
