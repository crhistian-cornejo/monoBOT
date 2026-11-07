//! A workstream takes on work that would keep a team busy for days: the user
//! gives a goal to an orchestrator, which splits it into areas, starts one
//! agent per area in its own worktree, keeps them coordinated, and returns the
//! work as pull requests. The orchestrator is a Mono of kind `workstream`: its
//! chat is the lead session, the sessions it starts are its agents.
//!
//! Ports `src/features/workstreams/model/*` and `src-tauri/src/workstreams.rs`.
//! The Mono roster entry keeps `kind: "workstream"` and a `workstream` object;
//! the caller owns that roster, so this module only reads and builds the
//! object, never the Mono around it.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The repository the agents work on, and where the orchestrator runs.
/// Serializes exactly as the desktop roster stores it (camelCase).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workstream {
    /// The repository the agents work on.
    pub project: String,
    /// Where the orchestrator runs: the project, or a worktree of its own.
    pub cwd: String,
    /// Names the agents' branches: `ws/<slug>/<area>`.
    pub slug: String,
    /// The shared folder for plans, notes and findings; code ships as PRs.
    pub folder: String,
    pub created_at: i64,
}

/// `parseWorkstream`: keeps only complete workstreams from storage. A missing
/// `cwd` is the project itself; a missing `createdAt` is 0.
pub fn parse(value: &Value) -> Option<Workstream> {
    let entry = value.as_object()?;
    let text = |key: &str| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let project = text("project")?;
    let cwd = text("cwd").unwrap_or_else(|| project.clone());
    Some(Workstream {
        project,
        cwd,
        slug: text("slug")?,
        folder: text("folder")?,
        created_at: entry
            .get("createdAt")
            .and_then(Value::as_f64)
            .map_or(0, |at| at as i64),
    })
}

/// The `kind: "workstream"` check of `mono.ts`' roster parser: the entry's
/// workstream when it is one and the object is complete. An entry of that kind
/// without a usable object is not a workstream (the desktop drops the kind).
pub fn from_roster_entry(entry: &Value) -> Option<Workstream> {
    (entry.get("kind")?.as_str()? == "workstream")
        .then(|| parse(entry.get("workstream")?))
        .flatten()
}

/// `isWorkstream` on a roster entry.
pub fn is_workstream(entry: &Value) -> bool {
    from_roster_entry(entry).is_some()
}

/// Latin letters with a diacritic, folded to their base letter. The desktop
/// strips the combining marks of the NFD form; this needs no extra crate.
fn fold(ch: char) -> Option<&'static str> {
    Some(match ch {
        'à'..='å' | 'À'..='Å' | 'ā' | 'ă' | 'ą' | 'Ā' | 'Ă' | 'Ą' => "a",
        'æ' | 'Æ' => "ae",
        'ç' | 'Ç' | 'ć' | 'ĉ' | 'ċ' | 'č' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' => "c",
        'ď' | 'đ' | 'Ď' | 'Đ' => "d",
        'è'..='ë' | 'È'..='Ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => {
            "e"
        }
        'ĝ' | 'ğ' | 'ġ' | 'ģ' | 'Ĝ' | 'Ğ' | 'Ġ' | 'Ģ' => "g",
        'ĥ' | 'ħ' | 'Ĥ' | 'Ħ' => "h",
        'ì'..='ï' | 'Ì'..='Ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' | 'İ' => {
            "i"
        }
        'ĵ' | 'Ĵ' => "j",
        'ķ' | 'Ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ł' | 'Ĺ' | 'Ļ' | 'Ľ' | 'Ł' => "l",
        'ñ' | 'Ñ' | 'ń' | 'ņ' | 'ň' | 'Ń' | 'Ņ' | 'Ň' => "n",
        'ò'..='ö' | 'Ò'..='Ö' | 'ø' | 'Ø' | 'ō' | 'ŏ' | 'ő' | 'Ō' | 'Ŏ' | 'Ő' => "o",
        'œ' | 'Œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' | 'Ŕ' | 'Ŗ' | 'Ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' | 'Ś' | 'Ŝ' | 'Ş' | 'Š' => "s",
        'ţ' | 'ť' | 'ŧ' | 'Ţ' | 'Ť' | 'Ŧ' => "t",
        'ù'..='ü'
        | 'Ù'..='Ü'
        | 'ũ'
        | 'ū'
        | 'ŭ'
        | 'ů'
        | 'ű'
        | 'ų'
        | 'Ũ'
        | 'Ū'
        | 'Ŭ'
        | 'Ů'
        | 'Ű'
        | 'Ų' => "u",
        'ŵ' | 'Ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' | 'Ý' | 'Ÿ' | 'Ŷ' => "y",
        'ź' | 'ż' | 'ž' | 'Ź' | 'Ż' | 'Ž' => "z",
        _ => return None,
    })
}

/// `workstreamSlug`: a branch-safe name from the workstream's name.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let push = |ch: char, out: &mut String| {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    };
    for ch in name.chars() {
        // Combining marks left over from a decomposed name vanish.
        if ('\u{300}'..='\u{36f}').contains(&ch) {
            continue;
        }
        match fold(ch) {
            Some(base) => base.chars().for_each(|c| push(c, &mut out)),
            None => push(ch, &mut out),
        }
    }
    out.truncate(40);
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "workstream".into()
    } else {
        out.into()
    }
}

/// `workstreamBranchPrefix`.
pub fn branch_prefix(slug: &str) -> String {
    format!("ws/{slug}/")
}

/// The branch a workstream's orchestrator works on when it has its own
/// worktree (`createWorkstream`).
pub fn orchestrator_branch(slug: &str) -> String {
    format!("{}orchestrator", branch_prefix(slug))
}

/// The branch the agent for one area ships from: `ws/<slug>/<area>`.
pub fn agent_branch(slug: &str, area: &str) -> String {
    let area = self::slug(area);
    format!("{}{area}", branch_prefix(slug))
}

/// `workstreamBrief`: what the orchestrator is told on top of the Mono rules
/// it already has.
pub fn brief(name: &str, workstream: &Workstream) -> String {
    let prefix = branch_prefix(&workstream.slug);
    let project = &workstream.project;
    let folder = &workstream.folder;
    format!(
        "<workstream>\n\
You are the orchestrator of the workstream \"{name}\" on the repository at {project}. You coordinate; you do not write the code yourself.\n\
- Wait for the user's goal. Agree with them on the outcome, how success is judged, the constraints, how the work is verified, and which decisions they want to make. Write the plan to {folder}/plan.md.\n\
- Split the work into independent areas and start one agent per area with app sessions.start, \"workspaceMode\":\"worktree\", in this project. Give each agent its instructions, the files it owns, the checks it must pass, and its branch: {prefix}<area>. Launch independent areas in the same turn so they run in parallel.\n\
- Each agent finishes by renaming its worktree branch to its {prefix}<area> branch, committing, pushing, and opening a draft pull request with gh pr create, then reports the PR URL to you.\n\
- Agents report to you. Ask the user only when a decision is theirs alone. Reuse agents that already know the code (app sessions.send) instead of starting new ones for follow-ups.\n\
- Plans, notes, findings and deliverables that are not code go in the shared folder {folder}; code ships only as pull requests.\n\
- When the user sets a standing rule (how PRs are opened, what to check), follow it for every agent from then on.\n\
</workstream>"
    )
}

/// The `monoWorkspace.ts` rule: an orchestrator's chat starts in its own
/// checkout, any other Mono at home.
pub fn chat_cwd<'a>(workstream: Option<&'a Workstream>, home: &'a str) -> &'a str {
    workstream.map_or(home, |workstream| workstream.cwd.as_str())
}

/// The shared folder's id check: ids end up in a path, so they stay in it.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Where a workstream's shared folder lives: `<app data>/workstreams/<id>`.
pub fn folder_path(data_dir: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("Invalid workstream id.".into());
    }
    Ok(data_dir.join("workstreams").join(id))
}

/// `workstream_folder`: creates the shared folder if needed and returns its
/// path with forward slashes, as the roster stores it on every platform.
pub fn ensure_folder(data_dir: &Path, id: &str) -> Result<String, String> {
    let path = folder_path(data_dir, id)?;
    std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path.to_string_lossy().replace('\\', "/"))
}

/// What the user gives when starting a workstream (`createWorkstream`'s input).
#[derive(Clone, Debug)]
pub struct NewWorkstream {
    pub name: String,
    pub project: String,
    /// The orchestrator gets a worktree of its own instead of the project.
    pub worktree: bool,
}

/// What `create` made. The caller adds the Mono: `id` is its id, `name` its
/// name, `projects` is `[workstream.project]`, `kind` "workstream".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    pub id: String,
    pub name: String,
    pub workstream: Workstream,
}

/// `createWorkstream` without the roster write: the workstream's shared
/// folder, and its orchestrator's checkout (the project itself, or a worktree
/// of its own on `ws/<slug>/orchestrator`).
pub fn create(data_dir: &Path, input: &NewWorkstream) -> Result<Created, String> {
    let name: String = input.name.trim().chars().take(120).collect();
    if name.is_empty() {
        return Err("Name the workstream".into());
    }
    let id = super::new_id();
    let slug = slug(&name);
    let folder = ensure_folder(data_dir, &id)?;
    let cwd = if input.worktree {
        crate::git::create_project_worktree(
            Path::new(&input.project),
            &orchestrator_branch(&slug),
            Some("HEAD"),
            false,
        )?
        .path
    } else {
        input.project.clone()
    };
    Ok(Created {
        id,
        name,
        workstream: Workstream {
            project: input.project.clone(),
            cwd,
            slug,
            folder,
            created_at: crate::time::now_ms() as i64,
        },
    })
}

/// A pull request opened from one of a workstream's branches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchPr {
    pub number: i64,
    pub title: String,
    pub url: String,
    /// OPEN, CLOSED or MERGED, as GitHub reports it.
    pub state: String,
    pub is_draft: bool,
    pub head_ref_name: String,
}

/// `prGroup`: Factory's grouping, what still needs the user first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrGroup {
    Open,
    Draft,
    Merged,
    Closed,
}
impl PrGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Draft => "Draft",
            Self::Merged => "Merged",
            Self::Closed => "Closed",
        }
    }
}
pub fn pr_group(pr: &BranchPr) -> PrGroup {
    match pr.state.as_str() {
        "MERGED" => PrGroup::Merged,
        "CLOSED" => PrGroup::Closed,
        _ if pr.is_draft => PrGroup::Draft,
        _ => PrGroup::Open,
    }
}

/// `branch_prs_with_prefix`: the pull requests of `gh pr list --json`'s output
/// whose head branch starts with `prefix`. Unreadable output is no pull
/// requests.
pub fn branch_prs_with_prefix(json: &str, prefix: &str) -> Vec<BranchPr> {
    serde_json::from_str::<Vec<BranchPr>>(json)
        .unwrap_or_default()
        .into_iter()
        .filter(|pr| pr.head_ref_name.starts_with(prefix))
        .collect()
}

/// `git_branch_prs`: the repository's pull requests whose head branch starts
/// with `prefix` (a workstream's `ws/<slug>/`), newest first.
pub fn branch_prs(project: &Path, prefix: &str) -> Result<Vec<BranchPr>, String> {
    if prefix.trim().is_empty() {
        return Err("A branch prefix is required".into());
    }
    if crate::files::remote::is_remote(project) {
        return Err("Pull requests are only listed for local projects.".into());
    }
    let json = crate::scm::gh_run(
        &crate::scm::expand_home(project),
        &[
            "pr",
            "list",
            "--state",
            "all",
            "--limit",
            "100",
            "--json",
            "number,title,url,state,isDraft,headRefName",
        ],
    )?;
    Ok(branch_prs_with_prefix(&json, prefix))
}

/// `listWorkstreamPrs`: the pull requests opened from the workstream's branches.
pub fn list_prs(workstream: &Workstream) -> Result<Vec<BranchPr>, String> {
    branch_prs(
        Path::new(&workstream.project),
        &branch_prefix(&workstream.slug),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Workstream {
        Workstream {
            project: "/repo".into(),
            cwd: "/repo-worktrees/ws-migrate".into(),
            slug: "migrate-to-react-19".into(),
            folder: "/data/workstreams/abc".into(),
            created_at: 1,
        }
    }

    #[test]
    fn turns_any_name_into_a_branch_safe_slug() {
        assert_eq!(slug("Migrar a React 19 — ¡ya!"), "migrar-a-react-19-ya");
        assert_eq!(slug("   "), "workstream");
        assert_eq!(slug(&"x".repeat(80)).len(), 40);
        assert_eq!(branch_prefix("api"), "ws/api/");
        // Diacritics fold, composed or decomposed; the cut never ends in "-".
        assert_eq!(slug("Migración ñandú"), "migracion-nandu");
        assert_eq!(slug("Cafe\u{301} Zürich"), "cafe-zurich");
        assert_eq!(slug(&format!("{} y", "a".repeat(39))), "a".repeat(39));
        assert_eq!(slug("¡¡!!"), "workstream");
        assert_eq!(orchestrator_branch("api"), "ws/api/orchestrator");
        assert_eq!(agent_branch("api", "Web UI"), "ws/api/web-ui");
    }

    #[test]
    fn keeps_only_complete_workstreams_from_storage() {
        let stored = serde_json::to_value(sample()).unwrap();
        assert_eq!(stored["createdAt"], 1);
        assert_eq!(parse(&stored), Some(sample()));
        let mut no_cwd = stored.clone();
        no_cwd["cwd"] = Value::Null;
        assert_eq!(parse(&no_cwd).unwrap().cwd, "/repo");
        no_cwd["cwd"] = json!("");
        assert_eq!(parse(&no_cwd).unwrap().cwd, "/repo");
        assert_eq!(parse(&json!({"project":"/repo"})), None);
        assert_eq!(parse(&Value::Null), None);
        assert_eq!(parse(&json!("text")), None);
        let mut odd = stored;
        odd["createdAt"] = json!("soon");
        assert_eq!(parse(&odd).unwrap().created_at, 0);
    }

    #[test]
    fn a_roster_entry_is_a_workstream_only_with_its_kind_and_a_complete_object() {
        let stored = serde_json::to_value(sample()).unwrap();
        let entry = json!({"id":"a","name":"Migrate","kind":"workstream","workstream":stored});
        assert_eq!(from_roster_entry(&entry), Some(sample()));
        assert!(is_workstream(&entry));
        assert!(!is_workstream(&json!({"kind":"bot","workstream":stored})));
        assert!(!is_workstream(&json!({"kind":"workstream"})));
        assert!(!is_workstream(
            &json!({"kind":"workstream","workstream":{"project":"/repo"}})
        ));
        assert_eq!(
            chat_cwd(Some(&sample()), "/home"),
            "/repo-worktrees/ws-migrate"
        );
        assert_eq!(chat_cwd(None, "/home"), "/home");
    }

    #[test]
    fn tells_the_orchestrator_to_coordinate_isolate_agents_and_ship_prs() {
        let text = brief("Migrate", &sample());
        assert!(text.starts_with("<workstream>\nYou are the orchestrator of the workstream \"Migrate\" on the repository at /repo."));
        assert!(text.ends_with("\n</workstream>"));
        assert!(text.contains("you do not write the code yourself"));
        assert!(text.contains("\"workspaceMode\":\"worktree\""));
        assert!(text.contains("ws/migrate-to-react-19/<area>"));
        assert!(text.contains("gh pr create"));
        assert!(text.contains("/data/workstreams/abc"));
        assert!(text.contains("Write the plan to /data/workstreams/abc/plan.md."));
    }

    #[test]
    fn groups_pull_requests_the_way_the_prs_tab_shows_them() {
        let pr = |state: &str, is_draft| BranchPr {
            number: 1,
            title: String::new(),
            url: String::new(),
            state: state.into(),
            is_draft,
            head_ref_name: "ws/a/b".into(),
        };
        assert_eq!(pr_group(&pr("OPEN", false)), PrGroup::Open);
        assert_eq!(pr_group(&pr("OPEN", true)), PrGroup::Draft);
        assert_eq!(pr_group(&pr("MERGED", false)), PrGroup::Merged);
        assert_eq!(pr_group(&pr("CLOSED", true)), PrGroup::Closed);
        assert_eq!(PrGroup::Draft.label(), "Draft");
    }

    #[test]
    fn workstream_prs_are_matched_by_branch_prefix() {
        let json = r#"[
            {"number":3,"title":"API","url":"u3","state":"OPEN","isDraft":true,"headRefName":"ws/migrate/api"},
            {"number":2,"title":"Other","url":"u2","state":"MERGED","isDraft":false,"headRefName":"feature/x"},
            {"number":1,"title":"Web","url":"u1","state":"MERGED","isDraft":false,"headRefName":"ws/migrate/web"}
        ]"#;
        let prs = branch_prs_with_prefix(json, "ws/migrate/");
        assert_eq!(
            prs.iter().map(|pr| pr.number).collect::<Vec<_>>(),
            vec![3, 1]
        );
        assert!(prs[0].is_draft);
        assert!(branch_prs_with_prefix("not json", "ws/").is_empty());
        assert!(branch_prs(Path::new("/nowhere"), "  ").is_err());
    }

    #[test]
    fn workstream_ids_stay_inside_the_folder() {
        assert!(valid_id("a1b2-c3"));
        assert!(!valid_id("../x"));
        assert!(!valid_id("a/b"));
        assert!(!valid_id(""));
        assert!(!valid_id(&"a".repeat(65)));
        let temp = tempfile::tempdir().unwrap();
        assert!(ensure_folder(temp.path(), "..").is_err());
        let folder = ensure_folder(temp.path(), "abc-1").unwrap();
        assert!(!folder.contains('\\'));
        assert!(Path::new(&folder).is_dir());
        assert_eq!(ensure_folder(temp.path(), "abc-1").unwrap(), folder);
    }

    fn repo(root: &Path) -> PathBuf {
        let cwd = root.join("project");
        std::fs::create_dir(&cwd).unwrap();
        let git = |args: &[&str]| crate::git::git(&cwd, args).unwrap();
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "Native test"]);
        git(&["config", "user.email", "native@example.invalid"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(cwd.join("a.txt"), "base\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "Base"]);
        cwd
    }

    #[test]
    fn creating_a_workstream_makes_its_folder_and_an_orchestrator_worktree() {
        let temp = tempfile::tempdir().unwrap();
        let project = repo(temp.path());
        let data = temp.path().join("data");
        let project_text = project.to_string_lossy().into_owned();
        let empty = NewWorkstream {
            name: "  ".into(),
            project: project_text.clone(),
            worktree: true,
        };
        assert_eq!(create(&data, &empty).unwrap_err(), "Name the workstream");

        let made = create(
            &data,
            &NewWorkstream {
                name: " Migrar a React 19 ".into(),
                project: project_text.clone(),
                worktree: true,
            },
        )
        .unwrap();
        assert_eq!(made.name, "Migrar a React 19");
        assert_eq!(made.workstream.slug, "migrar-a-react-19");
        assert!(valid_id(&made.id));
        assert_eq!(made.workstream.project, project_text);
        assert_ne!(made.workstream.cwd, project_text);
        assert!(Path::new(&made.workstream.cwd).join("a.txt").is_file());
        assert!(Path::new(&made.workstream.folder).is_dir());
        assert!(made
            .workstream
            .folder
            .ends_with(&format!("workstreams/{}", made.id)));
        assert!(made.workstream.created_at > 0);
        let trees = crate::git::worktrees(&project).unwrap();
        assert!(trees
            .iter()
            .any(|tree| tree.branch.as_deref() == Some("ws/migrar-a-react-19/orchestrator")));
        // The stored object reads back as the same workstream.
        let stored = serde_json::to_value(&made.workstream).unwrap();
        assert_eq!(parse(&stored), Some(made.workstream.clone()));

        // The same name again finds the branch taken rather than reusing it.
        let again = create(
            &data,
            &NewWorkstream {
                name: "Migrar a React 19".into(),
                project: project_text.clone(),
                worktree: true,
            },
        );
        assert!(again.is_err());

        let plain = create(
            &data,
            &NewWorkstream {
                name: "Plain".into(),
                project: project_text.clone(),
                worktree: false,
            },
        )
        .unwrap();
        assert_eq!(plain.workstream.cwd, project_text);
    }
}
