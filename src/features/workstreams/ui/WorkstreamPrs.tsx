import { useCallback, useEffect, useState } from "react";
import { GitPullRequest, RefreshCw } from "../../../shared/ui/icons";
import { EmptyState } from "../../monos/ui/monoPanelParts";
import { UserLinkPreview } from "../../sessions/ui/UserLinkPreview";
import {
  listWorkstreamPrs,
  prGroup,
  type WorkstreamPr,
} from "../model/workstreamApi";
import { workstreamBranchPrefix, type Workstream } from "../model/workstreams";

const GROUPS = ["Open", "Draft", "Merged", "Closed"] as const;

function prLink(pr: WorkstreamPr) {
  const match = /github\.com\/([^/]+\/[^/]+)\/pull\/(\d+)/.exec(pr.url);
  return {
    url: pr.url,
    host: "github.com",
    displayUrl: pr.url.replace(/^https?:\/\//, ""),
    ...(match
      ? {
          githubWorkItem: {
            kind: "pr" as const,
            repo: match[1]!,
            number: Number(match[2]),
          },
        }
      : {}),
  };
}

/** The pull requests the workstream's agents opened, by where they stand. */
export function WorkstreamPrs({ workstream }: { workstream: Workstream }) {
  const [prs, setPrs] = useState<WorkstreamPr[] | null>(null);
  const [error, setError] = useState<string>();
  const { project, slug } = workstream;
  const load = useCallback(() => {
    setError(undefined);
    listWorkstreamPrs({ ...workstream, project, slug })
      .then(setPrs)
      .catch((reason: unknown) => {
        setPrs([]);
        setError(String(reason));
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed by value
  }, [project, slug]);
  useEffect(() => {
    load();
    // Agents open PRs as they finish; keep the list current while it shows.
    const timer = window.setInterval(load, 60_000);
    return () => window.clearInterval(timer);
  }, [load]);

  const refresh = (
    <button
      type="button"
      onClick={load}
      className="ml-auto flex items-center gap-1 px-1.5 py-0.5 text-[11px] text-content/55 hover:bg-content/6 hover:text-content"
    >
      <RefreshCw className="size-3" strokeWidth={1.75} />
      Refresh
    </button>
  );

  if (error)
    return (
      <div className="flex flex-col gap-2">
        <div className="flex">{refresh}</div>
        <p role="alert" className="px-1 text-xs text-red-400">
          {error}
        </p>
      </div>
    );
  if (prs && prs.length === 0)
    return (
      <EmptyState icon={GitPullRequest}>
        Each agent opens a pull request from its{" "}
        {workstreamBranchPrefix(workstream.slug)}… branch when its part is done,
        and it shows up here for review.
      </EmptyState>
    );

  return (
    <div className="flex flex-col gap-1" data-workstream-prs>
      <div className="flex">{refresh}</div>
      {GROUPS.map((group) => {
        const items = (prs ?? []).filter((pr) => prGroup(pr) === group);
        if (!items.length) return null;
        return (
          <section key={group} className="flex flex-col gap-1">
            <h3 className="px-1 pt-2 text-[12px] font-medium text-content/50">
              {group} · {items.length}
            </h3>
            {items.map((pr) => (
              <UserLinkPreview
                key={pr.number}
                link={prLink(pr)}
                cwd={workstream.project}
              />
            ))}
          </section>
        );
      })}
    </div>
  );
}
