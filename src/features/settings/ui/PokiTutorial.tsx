import { useState, type CSSProperties, type ReactNode } from "react";
import { copyText } from "../../../platform/tauri/clipboard";
import { BetaBadge } from "../../../shared/ui/BetaBadge";
import { Modal } from "../../../shared/ui/Modal";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { Check, Copy } from "../../../shared/ui/icons";
import { POKI_STATUS_TEXT, type PokiStatus } from "../model/pokiSetup";
import { POKI_REPO, POKI_STEPS, type PokiScene } from "../model/pokiTutorial";

/** A CSS variable on an element, for the scenes' staggered motion. */
const vars = (values: Record<string, string | number>) =>
  values as CSSProperties;

/** Step by step, with a moving picture of each step, how to get the
 * iPhone connector working. The pictures are mock-ups, not the real apps. */
export function PokiTutorial({
  status,
  onClose,
}: {
  status: PokiStatus | null;
  onClose: () => void;
}) {
  const [index, setIndex] = useState(0);
  const step = POKI_STEPS[index]!;
  const last = index === POKI_STEPS.length - 1;
  return (
    <Modal
      title="Set up your iPhone"
      description="Poki connector, experimental, macOS only"
      size="lg"
      fitViewport
      onClose={onClose}
    >
      <div className="grid sm:grid-cols-[190px_minmax(0,1fr)]">
        <ol className="hidden space-y-0.5 border-r border-stroke p-2 sm:block">
          {POKI_STEPS.map((entry, at) => (
            <li key={entry.scene}>
              <button
                type="button"
                onClick={() => setIndex(at)}
                aria-current={at === index ? "step" : undefined}
                className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] ${
                  at === index
                    ? "bg-selection text-content"
                    : "text-content/60 hover:bg-content/8"
                }`}
              >
                <span
                  className={`flex size-4.5 shrink-0 items-center justify-center rounded-full text-[10px] ${
                    at < index
                      ? "bg-accent text-background-base"
                      : at === index
                        ? "border border-accent text-accent"
                        : "border border-content/20"
                  }`}
                >
                  {at < index ? <Check className="size-3" /> : at + 1}
                </span>
                <span className="truncate">{entry.title}</span>
              </button>
            </li>
          ))}
        </ol>

        <div className="min-w-0 space-y-4 px-5 py-4">
          <div
            key={step.scene}
            className="poki-stage flex h-52 items-center justify-center overflow-hidden rounded-lg border border-stroke bg-content/[0.03]"
            aria-hidden
          >
            <Scene scene={step.scene} status={status} />
          </div>

          <div className="space-y-1.5">
            <p className="flex items-center gap-2 text-[11px] uppercase tracking-wide text-content/45">
              Step {index + 1} of {POKI_STEPS.length}
              {index === 0 ? <BetaBadge label="Experimental" /> : null}
            </p>
            <h3 className="text-[15px] font-semibold">{step.title}</h3>
            <p className="text-[13px] leading-relaxed text-content/75">
              {step.body}
            </p>
          </div>

          {step.commands ? <Commands lines={step.commands} /> : null}

          {step.scene === "folder" ? (
            <p className="text-[12px] text-content/60">
              Right now:{" "}
              <span className="font-medium text-content">
                {status ? POKI_STATUS_TEXT[status].label : "checking…"}
              </span>
            </p>
          ) : null}

          {step.points?.length ? (
            <ul className="space-y-1 text-[12px] text-content/60">
              {step.points.map((point) => (
                <li key={point} className="flex gap-2">
                  <span className="mt-1.5 size-1 shrink-0 rounded-full bg-content/40" />
                  <span>{point}</span>
                </li>
              ))}
            </ul>
          ) : null}

          <div className="flex items-center justify-between gap-3 border-t border-stroke pt-3">
            <a
              href={POKI_REPO}
              target="_blank"
              rel="noreferrer"
              className="truncate text-[12px] text-content/55 hover:text-content hover:underline"
            >
              Poki on GitHub (AGPL-3.0)
            </a>
            <div className="flex shrink-0 items-center gap-2">
              <button
                type="button"
                disabled={index === 0}
                onClick={() => setIndex(index - 1)}
                className="rounded-md px-3 py-1.5 text-[12px] hover:bg-content/8 disabled:opacity-40 disabled:hover:bg-transparent"
              >
                Back
              </button>
              <button
                type="button"
                onClick={() => (last ? onClose() : setIndex(index + 1))}
                className="primary-action rounded-md px-3 py-1.5 text-[12px] font-medium"
              >
                {last ? "Done" : "Next"}
              </button>
            </div>
          </div>
        </div>
      </div>
    </Modal>
  );
}

function Commands({ lines }: { lines: readonly string[] }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="flex items-start justify-between gap-3 rounded-md border border-content/10 bg-background-base px-3 py-2">
      <pre className="min-w-0 overflow-x-auto font-mono text-[12px] leading-relaxed">
        {lines.map((line) => (
          <div key={line}>
            <span className="select-none text-content/35">$ </span>
            {line}
          </div>
        ))}
      </pre>
      <SecondaryButton
        aria-label="Copy the commands"
        onClick={() => {
          void copyText(lines.join("\n")).then(() => {
            setCopied(true);
            window.setTimeout(() => setCopied(false), 1500);
          });
        }}
      >
        {copied ? (
          <Check className="size-3.5" />
        ) : (
          <Copy className="size-3.5" />
        )}
        {copied ? "Copied" : "Copy"}
      </SecondaryButton>
    </div>
  );
}

function Scene({
  scene,
  status,
}: {
  scene: PokiScene;
  status: PokiStatus | null;
}) {
  switch (scene) {
    case "needs":
      return <NeedsScene />;
    case "clone":
      return (
        <TerminalScene
          lines={[
            { type: "git clone …/poki.git ~/poki" },
            { type: "cd ~/poki" },
            { type: "./bin/setup" },
            { out: "✓ Xcode  ✓ Node 24  ✓ Rust  ✓ ffmpeg" },
            { out: "Building the native bridge…" },
          ]}
        />
      );
    case "cable":
      return <CableScene />;
    case "developer":
      return <DeveloperScene />;
    case "start":
      return (
        <TerminalScene
          lines={[
            { type: "npm start" },
            { out: '{"event":"ready","url":"http://127.0.0.1:47825/mcp"}' },
          ]}
          footer="Keep this window open"
        />
      );
    case "folder":
      return <FolderScene ready={status === "ready"} />;
    case "bot":
      return <BotScene />;
  }
}

function NeedsScene() {
  const items = ["macOS 15+", "Xcode", "Node 24", "iPhone + cable"];
  return (
    <ul className="grid grid-cols-2 gap-2.5">
      {items.map((item, at) => (
        <li
          key={item}
          className="poki-pop flex items-center gap-2 rounded-md border border-stroke bg-background-base px-3 py-2 text-[12px]"
          style={vars({ "--poki-delay": `${at * 220}ms` })}
        >
          <span
            className="poki-tick flex size-4 items-center justify-center rounded-full bg-accent text-background-base"
            style={vars({ "--poki-delay": `${at * 220 + 260}ms` })}
          >
            <Check className="size-3" />
          </span>
          {item}
        </li>
      ))}
    </ul>
  );
}

type TerminalLine = { type: string } | { out: string };

/** A terminal that types each command, then shows what it prints. */
function TerminalScene({
  lines,
  footer,
}: {
  lines: readonly TerminalLine[];
  footer?: string;
}) {
  let delay = 200;
  return (
    <div className="w-[min(420px,90%)] overflow-hidden rounded-md border border-content/15 bg-background-base shadow-sm">
      <div className="flex gap-1.5 border-b border-stroke px-2.5 py-1.5">
        {[0, 1, 2].map((dot) => (
          <span key={dot} className="size-2 rounded-full bg-content/20" />
        ))}
      </div>
      <div className="space-y-1 px-3 py-2.5 font-mono text-[11px] leading-snug">
        {lines.map((line, at) => {
          const start = delay;
          if ("type" in line) {
            delay += line.type.length * 45 + 350;
            return (
              <div key={at} className="flex">
                <span className="text-accent">$&nbsp;</span>
                <span
                  className="poki-type"
                  style={vars({
                    "--poki-chars": line.type.length,
                    "--poki-delay": `${start}ms`,
                    "--poki-duration": `${line.type.length * 45}ms`,
                  })}
                >
                  {line.type}
                </span>
              </div>
            );
          }
          delay += 300;
          return (
            <div
              key={at}
              className="poki-pop truncate text-content/60"
              style={vars({ "--poki-delay": `${start}ms` })}
            >
              {line.out}
            </div>
          );
        })}
        <span
          className="poki-caret inline-block h-3 w-1.5 bg-content/60 align-middle"
          style={vars({ "--poki-delay": `${delay}ms` })}
        />
      </div>
      {footer ? (
        <div
          className="poki-pop border-t border-stroke px-3 py-1.5 text-[11px] text-accent"
          style={vars({ "--poki-delay": `${delay}ms` })}
        >
          {footer}
        </div>
      ) : null}
    </div>
  );
}

/** A Mac and an iPhone joined by a cable, the phone asking to trust it. */
function CableScene() {
  return (
    <svg viewBox="0 0 360 180" className="h-full max-h-48 w-full max-w-md">
      {/* Mac */}
      <rect
        x="24"
        y="40"
        width="150"
        height="96"
        rx="6"
        className="fill-background-base stroke-content/40"
        strokeWidth="2"
      />
      <rect
        x="34"
        y="50"
        width="130"
        height="76"
        rx="2"
        className="fill-content/5"
      />
      <path
        d="M12 140 h174 l-8 10 h-158 z"
        className="fill-content/15 stroke-content/40"
        strokeWidth="1.5"
      />
      <text
        x="99"
        y="92"
        textAnchor="middle"
        className="fill-content/50 font-mono text-[9px]"
      >
        npm start
      </text>
      {/* Cable */}
      <path
        id="poki-cable"
        d="M186 120 C 230 120, 236 166, 274 160"
        fill="none"
        className="stroke-content/35"
        strokeWidth="3"
        strokeLinecap="round"
      />
      <path
        d="M186 120 C 230 120, 236 166, 274 160"
        fill="none"
        className="poki-current stroke-accent"
        strokeWidth="3"
        strokeLinecap="round"
        pathLength={100}
      />
      {/* iPhone */}
      <g className="poki-phone">
        <rect
          x="262"
          y="22"
          width="76"
          height="140"
          rx="12"
          className="fill-background-base stroke-content/50"
          strokeWidth="2"
        />
        <rect
          x="288"
          y="28"
          width="24"
          height="6"
          rx="3"
          className="fill-content/30"
        />
        {/* Trust alert */}
        <g className="poki-alert">
          <rect
            x="268"
            y="64"
            width="64"
            height="58"
            rx="6"
            className="fill-content/10"
          />
          <text
            x="300"
            y="78"
            textAnchor="middle"
            className="fill-content text-[6.5px] font-semibold"
          >
            Trust This
          </text>
          <text
            x="300"
            y="86"
            textAnchor="middle"
            className="fill-content text-[6.5px] font-semibold"
          >
            Computer?
          </text>
          <line
            x1="268"
            y1="94"
            x2="332"
            y2="94"
            className="stroke-content/20"
          />
          <text
            x="300"
            y="104"
            textAnchor="middle"
            className="fill-accent text-[7px] font-semibold"
          >
            Trust
          </text>
          <line
            x1="268"
            y1="109"
            x2="332"
            y2="109"
            className="stroke-content/20"
          />
          <text
            x="300"
            y="118"
            textAnchor="middle"
            className="fill-content/50 text-[6.5px]"
          >
            Don’t Trust
          </text>
          <circle
            cx="300"
            cy="101"
            r="9"
            fill="none"
            className="poki-tap stroke-accent"
            strokeWidth="1.5"
          />
        </g>
      </g>
    </svg>
  );
}

/** A phone's settings, with Developer Mode and UI Automation turning on. */
function DeveloperScene() {
  const rows: { label: string; detail: string; delay: number }[] = [
    {
      label: "Developer Mode",
      detail: "Privacy & Security",
      delay: 700,
    },
    { label: "UI Automation", detail: "Developer", delay: 1900 },
  ];
  return (
    <Phone>
      <p className="px-2.5 pt-2 text-[10px] font-semibold">Settings</p>
      <div className="mt-1.5 space-y-1.5 px-1.5">
        {rows.map((row) => (
          <div
            key={row.label}
            className="poki-pop rounded-md bg-content/8 px-2 py-1.5"
            style={vars({ "--poki-delay": `${row.delay - 500}ms` })}
          >
            <p className="text-[8px] text-content/45">{row.detail}</p>
            <div className="flex items-center justify-between gap-2">
              <span className="text-[9.5px]">{row.label}</span>
              <Toggle delay={row.delay} />
            </div>
          </div>
        ))}
        <p
          className="poki-pop px-1 text-[8px] text-content/45"
          style={vars({ "--poki-delay": "2600ms" })}
        >
          Restart when asked.
        </p>
      </div>
    </Phone>
  );
}

function Toggle({ delay }: { delay: number }) {
  return (
    <span
      className="poki-toggle relative inline-flex h-3.5 w-6 shrink-0 rounded-full"
      style={vars({ "--poki-delay": `${delay}ms` })}
    >
      <span className="poki-knob absolute left-0.5 top-0.5 size-2.5 rounded-full bg-background-base shadow-sm" />
    </span>
  );
}

/** MonoBOT's own setting being filled in, its status turning to Ready. */
function FolderScene({ ready }: { ready: boolean }) {
  const path = "/Users/you/poki";
  return (
    <div className="w-[min(380px,90%)] space-y-2.5 rounded-md border border-stroke bg-background-base p-3">
      <div className="flex items-center justify-between">
        <span className="text-[12px] font-medium">iPhone (Poki)</span>
        <Toggle delay={1300} />
      </div>
      <div className="flex h-8 items-center rounded-md border border-content/10 px-2.5 font-mono text-[12px]">
        <span
          className="poki-type"
          style={vars({
            "--poki-chars": path.length,
            "--poki-delay": "200ms",
            "--poki-duration": `${path.length * 50}ms`,
          })}
        >
          {path}
        </span>
      </div>
      <div className="relative h-4 text-[12px]">
        <span className="poki-out absolute inset-0 flex items-center gap-2 text-content/55">
          <span className="size-2 animate-pulse rounded-full bg-content/30" />
          Checking…
        </span>
        <span className="poki-in absolute inset-0 flex items-center gap-2">
          <span className="size-2 rounded-full bg-diff-add" />
          {ready ? "Ready" : "Ready (once Poki runs)"}
        </span>
      </div>
    </div>
  );
}

/** The Phone bot's chat: it reads, asks before a tap, then reads again. */
function BotScene() {
  return (
    <div className="w-[min(400px,92%)] space-y-2 text-[11px]">
      <Bubble delay={150}>Reading the screen: Settings is open.</Bubble>
      <div
        className="poki-pop space-y-2 rounded-md border border-content/15 bg-background-base p-2.5"
        style={vars({ "--poki-delay": "900ms" })}
      >
        <p>
          Phone wants to <span className="font-medium">tap “Wi-Fi”</span>
        </p>
        <div className="flex justify-end gap-1.5">
          <span className="rounded px-2 py-0.5 text-content/60">Deny</span>
          <span
            className="poki-press primary-action rounded px-2 py-0.5 font-medium"
            style={vars({ "--poki-delay": "1900ms" })}
          >
            Allow
          </span>
        </div>
      </div>
      <Bubble delay={2600}>Wi-Fi is on and connected to Home.</Bubble>
    </div>
  );
}

function Bubble({ delay, children }: { delay: number; children: ReactNode }) {
  return (
    <p
      className="poki-pop w-fit max-w-[85%] rounded-lg bg-content/8 px-2.5 py-1.5"
      style={vars({ "--poki-delay": `${delay}ms` })}
    >
      {children}
    </p>
  );
}

function Phone({ children }: { children: ReactNode }) {
  return (
    <div className="relative h-48 w-28 overflow-hidden rounded-[18px] border-2 border-content/40 bg-background-base">
      <span className="absolute left-1/2 top-1.5 h-1.5 w-7 -translate-x-1/2 rounded-full bg-content/30" />
      <div className="pt-3">{children}</div>
    </div>
  );
}
