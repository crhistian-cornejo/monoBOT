import type { ReactNode } from "react";

/**
 * A small page for a file, with its format on a banner and a few lines that
 * suggest what is inside (text, a table, slides, code). It stands for a file
 * the user can open, in lists and headers. Colours are the app's tokens, so it
 * follows the theme and the accent the user picked.
 */

export type FileFormat =
  | "doc"
  | "pdf"
  | "md"
  | "mdx"
  | "csv"
  | "xls"
  | "xlsx"
  | "txt"
  | "ppt"
  | "pptx"
  | "zip"
  | "rar"
  | "tar"
  | "gz"
  | "code"
  | "html"
  | "js"
  | "jsx"
  | "tsx"
  | "css"
  | "json"
  | "img"
  | "png"
  | "jpg"
  | "jpeg"
  | "video";

const FORMATS = new Set<string>([
  "doc",
  "pdf",
  "md",
  "mdx",
  "csv",
  "xls",
  "xlsx",
  "txt",
  "ppt",
  "pptx",
  "zip",
  "rar",
  "tar",
  "gz",
  "code",
  "html",
  "js",
  "jsx",
  "tsx",
  "css",
  "json",
  "img",
  "png",
  "jpg",
  "jpeg",
  "video",
]);

const ALIASES: Record<string, FileFormat> = {
  docx: "doc",
  rtf: "doc",
  markdown: "md",
  htm: "html",
  mjs: "js",
  cjs: "js",
  ts: "tsx",
  gif: "img",
  webp: "img",
  svg: "img",
  mp4: "video",
  mov: "video",
  webm: "video",
  tgz: "gz",
};

/** The format a file name or path stands for; plain text when it is not known. */
export function fileFormatOf(name: string): FileFormat {
  const extension = name.split(/[?#]/)[0].split(".").pop()?.toLowerCase() ?? "";
  if (FORMATS.has(extension)) return extension as FileFormat;
  return ALIASES[extension] ?? "txt";
}

/** The PDF takes the accent; the rest read as quieter ink on the theme. */
function banner(format: FileFormat): string {
  return format === "pdf"
    ? "bg-accent text-white"
    : "bg-content/65 text-background-base";
}

function Bar({ className }: { className: string }) {
  return <div className={`h-0.5 rounded-full ${className}`} />;
}

/** Rows of bars, each a list of widths: how lines of text look from afar. */
function Lines({ rows }: { rows: string[][] }) {
  return (
    <div className="space-y-1.5">
      {rows.map((row, index) => (
        <div key={index} className="flex gap-1">
          {row.map((width, at) => (
            <Bar
              key={at}
              className={`${width} ${index === 0 && at === 0 ? "bg-content/20" : "bg-content/10"}`}
            />
          ))}
        </div>
      ))}
    </div>
  );
}

function Grid({ rows, strong }: { rows: number[]; strong?: boolean }) {
  return (
    <div className="space-y-0.5">
      {rows.map((cells, index) => (
        <div key={index} className="grid grid-cols-3 gap-0.5">
          {Array.from({ length: cells }, (_, at) => (
            <div
              key={at}
              className={`h-2 ${strong && index === 0 ? "bg-content/20" : "bg-content/5"}`}
            />
          ))}
        </div>
      ))}
    </div>
  );
}

function Mono({ children }: { children: ReactNode }) {
  return <div className="font-mono text-[5px] text-content/30">{children}</div>;
}

function Tag({ width, tone }: { width: string; tone: string }) {
  return <div className={`h-0.75 rounded-full ${width} ${tone}`} />;
}

function placeholder(format: FileFormat): ReactNode {
  switch (format) {
    case "md":
    case "mdx":
      return (
        <div className="space-y-1.5">
          <div className="flex items-center gap-1">
            <div className="text-[10px] font-bold text-content/30">#</div>
            <Bar className="w-6 bg-content/20" />
          </div>
          <div className="space-y-1">
            <Bar className="w-1/3 bg-content/10" />
            <Bar className="w-7 bg-content/10" />
          </div>
          <div className="space-y-1">
            <Bar className="w-8 bg-content/10" />
            <Bar className="w-4 bg-content/10" />
            <Bar className="w-1/3 bg-content/10" />
          </div>
        </div>
      );
    case "xls":
    case "xlsx":
      return <Grid rows={[3, 6, 2, 1]} strong />;
    case "csv":
      return (
        <>
          <div className="mb-2 grid grid-cols-3 gap-0.5">
            {[0, 1, 2].map((cell) => (
              <div key={cell} className="h-1.5 rounded-full bg-content/20" />
            ))}
          </div>
          <div className="space-y-1.5">
            {[3, 3, 2, 1].map((cells, row) => (
              <div key={row} className="grid grid-cols-3 gap-0.5">
                {Array.from({ length: cells }, (_, cell) => (
                  <div key={cell} className="h-1 rounded-full bg-content/5" />
                ))}
              </div>
            ))}
          </div>
        </>
      );
    case "zip":
    case "rar":
    case "tar":
    case "gz":
      return (
        <div className="relative flex h-full flex-col items-center justify-center">
          {Array.from({ length: 9 }, (_, row) => (
            <div key={row} className="flex overflow-hidden rounded-full">
              <div
                className={`size-1.5 ${row % 2 ? "bg-content/5" : "bg-content/20"}`}
              />
              <div
                className={`size-1.5 ${row % 2 ? "bg-content/20" : "bg-content/5"}`}
              />
            </div>
          ))}
        </div>
      );
    case "ppt":
    case "pptx":
      return (
        <>
          <div className="mb-1.5 space-y-1 rounded border border-content/10 bg-content/5 p-1">
            <div className="flex justify-center gap-1">
              <div className="size-3 rounded-sm bg-accent/40" />
            </div>
            <div className="mx-auto h-0.75 w-8 rounded-full bg-content/15" />
          </div>
          <div className="mb-1 flex justify-center gap-1">
            <div className="h-0.75 w-8 rounded-full bg-content/15" />
            <div className="h-0.75 w-4 rounded-full bg-content/15" />
          </div>
          <div className="space-y-1">
            <div className="h-0.75 w-4 rounded-full bg-content/15" />
            <div className="h-0.75 w-5 rounded-full bg-content/15" />
          </div>
        </>
      );
    case "img":
    case "png":
    case "jpg":
    case "jpeg":
      return (
        <div className="mb-1.5 space-y-1 rounded border border-content/10 bg-content/5 p-1">
          <div className="flex justify-center gap-1">
            <div className="size-3 rounded-sm bg-accent/40" />
          </div>
          <div className="mx-auto mt-1 h-0.75 w-4 rounded-full bg-content/15" />
          <div className="mx-auto h-0.75 w-8 rounded-full bg-content/15" />
        </div>
      );
    case "video":
      return (
        <div className="mb-1.5 space-y-1 rounded border border-content/10 bg-content/5 p-1">
          <div className="flex justify-center gap-1">
            <div className="size-0 border-y-[5px] border-l-8 border-y-transparent border-l-accent/60" />
          </div>
          <div className="mx-auto mt-1 h-0.75 w-4 rounded-full bg-content/15" />
          <div className="mx-auto h-0.75 w-8 rounded-full bg-content/15" />
        </div>
      );
    case "html":
    case "js":
    case "jsx":
    case "tsx":
    case "code":
      return (
        <div className="space-y-1">
          <div className="flex items-center gap-0.5">
            <Mono>&lt;</Mono>
            <Tag width="w-3" tone="bg-accent/60" />
            <Mono>&gt;</Mono>
          </div>
          <div className="flex items-center gap-0.5 pl-1">
            <Mono>&lt;</Mono>
            <Tag width="w-2.5" tone="bg-content/40" />
            <Mono>&gt;</Mono>
          </div>
          <div className="flex items-center gap-0.5 pl-1">
            <Mono>&lt;/</Mono>
            <Tag width="w-2.5" tone="bg-content/40" />
            <Mono>&gt;</Mono>
          </div>
          <div className="flex items-center gap-0.5">
            <Mono>&lt;</Mono>
            <Tag width="w-1" tone="bg-accent/60" />
            <Mono>/&gt;</Mono>
          </div>
        </div>
      );
    case "css":
    case "json": {
      const tone = format === "css" ? "bg-accent/50" : "bg-content/20";
      return (
        <div className="space-y-1">
          <div className="font-mono text-[6px] text-content/40">{"{"}</div>
          {[
            ["w-3", "w-4"],
            ["w-4", "w-2"],
            ["w-3", "w-4"],
          ].map((row, index) => (
            <div key={index} className="flex items-center gap-1 pl-1.5">
              {row.map((width) => (
                <Tag
                  key={width}
                  width={width}
                  tone={index === 0 ? tone : "bg-content/10"}
                />
              ))}
            </div>
          ))}
          <div className="font-mono text-[6px] text-content/40">{"}"}</div>
        </div>
      );
    }
    default:
      return (
        <Lines
          rows={[
            ["w-1/2"],
            ["w-1/3", "w-1/3"],
            ["w-1/2", "w-1/3"],
            ["w-1/3", "w-1/3"],
            ["w-1/3", "w-1/2"],
            ["w-1/3"],
          ]}
        />
      );
  }
}

/** The card is 56 by 72 at "md"; "sm" is the same drawing at about 40%, for rows and chips. */
const SCALE = 0.4;

export function FileCard({
  format,
  size = "md",
  className = "",
}: {
  format: FileFormat;
  size?: "md" | "sm";
  className?: string;
}) {
  const card = (
    <div aria-hidden className="relative size-fit">
      <div
        className={`absolute -right-2 bottom-1.5 z-2 rounded px-1.5 py-0.5 text-[8px] font-medium uppercase ${banner(format)}`}
      >
        {format}
      </div>
      <div className="relative z-1 h-18 w-14 space-y-3 rounded-md bg-background-base p-2 ring-1 ring-content/15">
        {placeholder(format)}
      </div>
    </div>
  );
  if (size === "md") return <div className={className}>{card}</div>;
  return (
    <div
      aria-hidden
      data-file-card={format}
      className={`relative shrink-0 ${className}`}
      style={{ width: 56 * SCALE, height: 72 * SCALE }}
    >
      <div
        className="absolute top-0 left-0 origin-top-left"
        style={{ transform: `scale(${SCALE})` }}
      >
        {card}
      </div>
    </div>
  );
}

export default FileCard;
