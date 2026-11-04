import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import { invalidateSkills, SKILLS_CHANGE_EVENT } from "../model/skills";

/** Anthropic's docx, xlsx, pptx and pdf skills. Their license does not allow
 * bundling them, so Claude Code installs them from Anthropic's marketplace. */
export function DocumentSkillsCard() {
  const [state, setState] = useState<
    | { kind: "idle" }
    | { kind: "busy" }
    | { kind: "done" }
    | { kind: "error"; message: string }
  >({ kind: "idle" });

  const install = () => {
    setState({ kind: "busy" });
    invoke<string>("install_document_skills")
      .then(() => {
        setState({ kind: "done" });
        invalidateSkills();
        window.dispatchEvent(new Event(SKILLS_CHANGE_EVENT));
      })
      .catch((error: unknown) =>
        setState({ kind: "error", message: String(error) }),
      );
  };

  return (
    <div className="mb-6 flex flex-wrap items-center justify-between gap-3 border border-stroke px-4 py-3">
      <div className="min-w-0">
        <p className="text-[13px] font-medium">Document skills by Anthropic</p>
        <p className="text-xs text-content/55">
          Create and read Word, Excel, PowerPoint and PDF files. Claude Code
          installs them for your user from the anthropics/skills marketplace.
        </p>
        {state.kind === "error" ? (
          <p className="mt-1 text-xs text-red-500">{state.message}</p>
        ) : null}
      </div>
      <button
        type="button"
        onClick={install}
        disabled={state.kind === "busy" || state.kind === "done"}
        className="h-7 shrink-0 border border-stroke px-3 text-xs hover:bg-content/5 disabled:opacity-60"
      >
        {state.kind === "busy"
          ? "Installing…"
          : state.kind === "done"
            ? "Installed"
            : "Install"}
      </button>
    </div>
  );
}
