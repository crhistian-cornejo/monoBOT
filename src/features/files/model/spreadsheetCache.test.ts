import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  MAX_CACHED,
  clearSpreadsheetCache,
  openSpreadsheet,
  type SpreadsheetIo,
} from "./spreadsheetCache";
import type { Workbook } from "./spreadsheet";

vi.mock("../../../platform/tauri/fs", () => ({
  readBinaryFile: vi.fn(),
  statFiles: vi.fn(),
}));
vi.mock("./spreadsheetWorker", () => ({ readSpreadsheet: vi.fn() }));

const book = (name: string): Workbook => ({
  sheets: [
    {
      name,
      rowCount: 1,
      colCount: 1,
      colWidths: [64],
      rowHeights: [20],
      cells: [],
      frozenRows: 0,
      frozenCols: 0,
      gridlines: true,
      truncated: false,
    },
  ],
});

function io(times: Record<string, number | null>) {
  return {
    stat: vi.fn(async (paths: string[]) =>
      paths.map((path) => ({ path, mtimeMs: times[path] ?? null })),
    ),
    read: vi.fn(async (path: string) => new TextEncoder().encode(path)),
    parse: vi.fn(async (_bytes: Uint8Array, path: string) => book(path)),
  } satisfies SpreadsheetIo;
}

const signal = () => new AbortController().signal;

beforeEach(() => clearSpreadsheetCache());

describe("openSpreadsheet", () => {
  it("reads a file once while it is unchanged, again once it changes", async () => {
    const times: Record<string, number | null> = { "/obra/LOB.xlsx": 1 };
    const disk = io(times);
    await openSpreadsheet("/obra/LOB.xlsx", signal(), disk);
    await openSpreadsheet("/obra/LOB.xlsx", signal(), disk);
    expect(disk.read).toHaveBeenCalledTimes(1);

    times["/obra/LOB.xlsx"] = 2;
    await openSpreadsheet("/obra/LOB.xlsx", signal(), disk);
    expect(disk.read).toHaveBeenCalledTimes(2);
  });

  it("shares one read between two openers, even when the first leaves", async () => {
    const disk = io({ "/obra/LOB.xlsx": 1 });
    const first = new AbortController();
    const leaving = openSpreadsheet("/obra/LOB.xlsx", first.signal, disk);
    first.abort();
    await expect(leaving).rejects.toThrow(/cancelled/);
    await expect(
      openSpreadsheet("/obra/LOB.xlsx", signal(), disk),
    ).resolves.toEqual(book("/obra/LOB.xlsx"));
    expect(disk.read).toHaveBeenCalledTimes(1);
  });

  it("does not keep files it cannot state, failed reads, or more than a few", async () => {
    const times: Record<string, number | null> = {};
    for (let index = 0; index <= MAX_CACHED; index++)
      times[`/b${index}.xlsx`] = 1;
    const disk = io(times);
    await openSpreadsheet("/unknown.xlsx", signal(), disk);
    await openSpreadsheet("/unknown.xlsx", signal(), disk);
    expect(disk.read).toHaveBeenCalledTimes(2);

    disk.parse.mockRejectedValueOnce(new Error("broken"));
    await expect(openSpreadsheet("/b0.xlsx", signal(), disk)).rejects.toThrow(
      "broken",
    );
    await openSpreadsheet("/b0.xlsx", signal(), disk);
    expect(disk.read).toHaveBeenCalledTimes(4);

    // Filling the cache pushes the oldest out.
    for (let index = 1; index <= MAX_CACHED; index++)
      await openSpreadsheet(`/b${index}.xlsx`, signal(), disk);
    disk.read.mockClear();
    await openSpreadsheet("/b0.xlsx", signal(), disk);
    await openSpreadsheet(`/b${MAX_CACHED}.xlsx`, signal(), disk);
    expect(disk.read.mock.calls.map(([path]) => path)).toEqual(["/b0.xlsx"]);
  });
});
