import { useSyncExternalStore } from "react";
import { Switch } from "../../../shared/ui/Switch";
import {
  enabledSkillPacksSnapshot,
  setSkillPackEnabled,
  SKILL_PACKS,
  subscribeSkillPacks,
} from "../model/skillPacks";
import { invalidateSkills, SKILLS_CHANGE_EVENT } from "../model/skills";

/**
 * The skill packs MonoBOT ships, each off until turned on. An enabled pack
 * reaches every new session; a bot gets one from its own tools.
 */
export function SkillPacksCard() {
  const enabled = useSyncExternalStore(
    subscribeSkillPacks,
    enabledSkillPacksSnapshot,
    () => [],
  );
  return (
    <section aria-labelledby="skill-packs-title" className="mb-6">
      <div className="pb-2">
        <h2 id="skill-packs-title" className="text-[13px] font-medium">
          Skill packs
        </h2>
        <p className="text-xs text-content/55">
          Skills from Claude Code and Codex that MonoBOT carries with it. Claude
          loads a pack you turn on in every new session; other agents follow a
          skill when you call it with /name. Give a bot a pack in its tools.
        </p>
      </div>
      <ul className="divide-y divide-stroke border border-stroke">
        {SKILL_PACKS.map((pack) => {
          const on = enabled.includes(pack.id);
          return (
            <li
              key={pack.id}
              className="flex items-center justify-between gap-4 px-4 py-2.5"
            >
              <div className="min-w-0">
                <p className="text-[13px] font-medium">
                  {pack.name}
                  <span className="ml-2 text-[11px] font-normal tabular-nums text-content/40">
                    {pack.skills.length} skills
                  </span>
                </p>
                <p className="text-xs text-content/55">{pack.description}</p>
                <p
                  className="truncate text-[11px] text-content/40"
                  title={pack.skills.join(", ")}
                >
                  {pack.skills.join(" · ")}
                  {pack.needs ? ` · needs ${pack.needs}` : ""}
                </p>
              </div>
              <Switch
                label={`${pack.name} skill pack`}
                on={on}
                onChange={(next) => {
                  setSkillPackEnabled(pack.id, next);
                  // The composer's /name list takes the pack in or out.
                  invalidateSkills();
                  window.dispatchEvent(new Event(SKILLS_CHANGE_EVENT));
                }}
              />
            </li>
          );
        })}
      </ul>
    </section>
  );
}
