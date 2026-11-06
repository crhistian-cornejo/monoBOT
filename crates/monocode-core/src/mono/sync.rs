//! Keeping the same bots on the user's computers through a folder they already
//! sync (OneDrive, iCloud Drive, Dropbox…). Each computer writes only its own
//! file there, so two never write the same one, and reads the others'. A bot
//! is compared three ways: here, on the other computer, and as both last
//! agreed. The decisions are src/features/bots/model/botSync.ts; the run is
//! botSyncIO.ts and the timing BotSyncRunner.tsx's.
//!
//! The folder's files are the desktop's (format [`FORMAT`]) and hash the same,
//! so the native app and the desktop app keep each other's bots. What this
//! computer remembers (the folder, its id, what both agreed, the last run) is
//! in the settings store.
use super::{
    transfer::{self, json::Json, Bot, Named, Travels},
    Roster,
};
use crate::store::Store;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

pub const FORMAT: &str = "monobot.bots.sync";
pub const VERSION: u32 = 1;
pub const FILE_PREFIX: &str = "monobot-bots-";
/// How often the other computers' files are read again.
pub const EVERY_MS: u64 = 2 * 60_000;
/// A change here is written out after this pause, so a burst writes once.
pub const SETTLE_MS: u64 = 5_000;
/// The shared folder (`monocode.botSyncFolder`).
pub const FOLDER_KEY: &str = "botSyncFolder";
/// This computer in the shared folder (`monocode.machineId`).
pub const MACHINE_KEY: &str = "machineId";
/// What both computers last agreed each bot was (`monocode.botSyncBase`).
pub const BASE_KEY: &str = "botSyncBase";
/// How the last run went (`monocode.botSyncLast`).
pub const LAST_KEY: &str = "botSyncLast";

/// Habit fields that change on every run: not part of what a bot is.
const RUN_FIELDS: [&str; 6] = [
    "nextRunAt",
    "lastRunAt",
    "lastOutcome",
    "lastError",
    "runs",
    "runRequested",
];

/// A bot in a computer's file: what it is, and its fingerprint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SyncedBot {
    #[serde(flatten)]
    pub bot: Bot,
    pub hash: String,
}

impl SyncedBot {
    /// The bot as compared and carried, with its fingerprint.
    pub fn new(bot: &Bot) -> Self {
        let bot = content(bot);
        Self {
            hash: hash(&bot),
            bot,
        }
    }
    pub fn sync_id(&self) -> &str {
        self.bot.sync_id.as_deref().unwrap_or_default()
    }
}

/// One computer's file in the shared folder (SyncFile).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFile {
    pub format: String,
    pub version: u32,
    pub machine: String,
    /// "Windows" or "Mac", for the user.
    pub machine_name: String,
    pub updated_at: u64,
    pub bots: Vec<SyncedBot>,
}

impl SyncFile {
    /// Another computer's file; none for anything else (parseSyncFile).
    pub fn parse(raw: &str) -> Option<Self> {
        let file = Json::parse(raw)?;
        if file.get("format")?.as_str()? != FORMAT
            || file.get("version")?.as_f64()? > f64::from(VERSION)
        {
            return None;
        }
        let machine = file.get("machine")?.as_str()?;
        let bots = file.get("bots")?.as_array()?;
        Some(Self {
            format: FORMAT.into(),
            version: file.get("version")?.as_f64()?.max(0.) as u32,
            machine: machine.into(),
            machine_name: file
                .get("machineName")
                .and_then(Json::as_str)
                .unwrap_or("another computer")
                .into(),
            updated_at: file
                .get("updatedAt")
                .and_then(Json::as_f64)
                .map_or(0, |at| at.max(0.) as u64),
            bots: bots
                .iter()
                .filter_map(|entry| {
                    let sync_id = entry.get("syncId")?.as_str()?;
                    let hash = entry.get("hash")?.as_str()?;
                    entry.get("name")?.as_str()?;
                    let mut bot = Bot::parse_json(entry)?;
                    bot.sync_id = Some(sync_id.into());
                    Some(SyncedBot {
                        bot,
                        hash: hash.into(),
                    })
                })
                .collect(),
        })
    }
    pub fn to_text(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }
}

/// The bot as compared and carried: its habits without their run state
/// (syncContent).
pub fn content(bot: &Bot) -> Bot {
    let Some(Json::Array(habits)) = bot.habits.as_deref().and_then(Json::parse) else {
        return bot.clone();
    };
    let steady = habits
        .into_iter()
        .map(|mut habit| {
            if matches!(habit, Json::Object(_)) {
                for field in RUN_FIELDS {
                    habit.remove(field);
                }
            }
            habit
        })
        .collect();
    Bot {
        habits: Some(Json::Array(steady).pretty() + "\n"),
        ..bot.clone()
    }
}

/// A fingerprint of what the bot is (not its identity): two FNV-1a passes
/// over its JSON with sorted keys, the same as the desktop's (botHash).
pub fn hash(bot: &Bot) -> String {
    let content = Bot {
        sync_id: None,
        ..content(bot)
    };
    let text: Vec<u16> = content.json().sorted().compact().encode_utf16().collect();
    let mut a: u32 = 0x811c_9dc5;
    let mut b: u32 = 0x0100_0193 ^ text.len() as u32;
    for code in text {
        a = (a ^ u32::from(code)).wrapping_mul(0x0100_0193);
        b = (b ^ u32::from(code)).wrapping_mul(0x5bd1_e995);
    }
    format!("{a:08x}{b:08x}")
}

/// A bot as the most recently written of the other computers' files has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remote {
    pub bot: SyncedBot,
    pub machine_name: String,
}

/// Each bot as the most recently written of the other computers' files has
/// it, in the order they were first seen (newestRemote).
pub fn newest_remote(files: &[SyncFile], machine: &str) -> Vec<Remote> {
    let mut newest: Vec<(Remote, u64)> = Vec::new();
    for file in files.iter().filter(|file| file.machine != machine) {
        for bot in &file.bots {
            let remote = Remote {
                bot: bot.clone(),
                machine_name: file.machine_name.clone(),
            };
            match newest
                .iter_mut()
                .find(|(known, _)| known.bot.sync_id() == bot.sync_id())
            {
                Some(known) if file.updated_at > known.1 => *known = (remote, file.updated_at),
                Some(_) => {}
                None => newest.push((remote, file.updated_at)),
            }
        }
    }
    newest.into_iter().map(|(remote, _)| remote).collect()
}

/// A bot here, as the sync sees it (LocalBot).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalBot {
    pub id: String,
    pub sync_id: String,
    pub name: String,
    pub bot_role: Option<String>,
    pub hash: String,
}

impl Named for LocalBot {
    fn name(&self) -> &str {
        &self.name
    }
    fn bot_role(&self) -> Option<&str> {
        self.bot_role.as_deref()
    }
}

/// Changed only there: this one becomes that one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Apply {
    pub local_id: String,
    pub bot: SyncedBot,
}

/// Changed on both: the user compares and decides (SyncConflict).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub local_id: String,
    pub bot: SyncedBot,
    pub machine_name: String,
}

/// A bot paired by role or name: its identity becomes the shared one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adopt {
    pub local_id: String,
    pub sync_id: String,
}

/// What to do with each bot (SyncPlan).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// New on the other computer: made here.
    pub create: Vec<SyncedBot>,
    pub apply: Vec<Apply>,
    pub conflicts: Vec<Conflict>,
    pub adopt: Vec<Adopt>,
    /// What both computers now agree each bot is, by sync id.
    pub agreed: BTreeMap<String, String>,
}

/// What to do with each bot (planBotSync). `base` is what both agreed each
/// bot was, by sync id. A bot deleted here, which both had agreed on, is not
/// brought back.
pub fn plan(local: &[LocalBot], remote: &[Remote], base: &BTreeMap<String, String>) -> Plan {
    let mut plan = Plan::default();
    let mut paired: HashSet<&str> = HashSet::new();
    for Remote { bot, machine_name } in remote {
        let there = bot.sync_id();
        let mut here = local.iter().find(|entry| entry.sync_id == there);
        if here.is_none() {
            // The first sync of two computers that each made "Research": they are one.
            let free = local.iter().filter(|entry| {
                !paired.contains(entry.id.as_str())
                    && !remote.iter().any(|r| r.bot.sync_id() == entry.sync_id)
            });
            here = transfer::existing_match(&bot.bot.name, bot.bot.bot_role.as_deref(), free);
            // Both sides pick the smaller id, so they settle on the same one.
            if let Some(here) = here.filter(|here| there < here.sync_id.as_str()) {
                plan.adopt.push(Adopt {
                    local_id: here.id.clone(),
                    sync_id: there.into(),
                });
            }
        }
        let Some(here) = here else {
            if base.get(there).is_none_or(String::is_empty) {
                plan.create.push(bot.clone());
            }
            continue;
        };
        paired.insert(&here.id);
        let sync_id = there.min(here.sync_id.as_str());
        let agreed = base
            .get(sync_id)
            .or_else(|| base.get(&here.sync_id))
            .or_else(|| base.get(there));
        let shared = || {
            let mut bot = bot.clone();
            bot.bot.sync_id = Some(sync_id.into());
            bot
        };
        if here.hash == bot.hash {
            plan.agreed.insert(sync_id.into(), bot.hash.clone());
        } else if agreed == Some(&here.hash) {
            plan.apply.push(Apply {
                local_id: here.id.clone(),
                bot: shared(),
            });
        } else if agreed == Some(&bot.hash) {
            // Changed here only: the other computer takes it.
        } else {
            plan.conflicts.push(Conflict {
                local_id: here.id.clone(),
                bot: shared(),
                machine_name: machine_name.clone(),
            });
        }
    }
    plan
}

// ---- The run: this computer's file, the others', and what changes here ----

/// How the last run went (SyncStatus).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub at: u64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<bool>,
}

fn setting(store: &Store, key: &str) -> Option<String> {
    store
        .setting(key)
        .ok()
        .flatten()
        .filter(|value| !value.is_empty())
}

pub fn load_folder(store: &Store) -> Option<String> {
    setting(store, FOLDER_KEY)
}

/// Chooses the shared folder, or stops syncing with none.
pub fn save_folder(store: &Store, folder: Option<&str>) -> Result<(), String> {
    store.save_setting(FOLDER_KEY, folder.unwrap_or_default())
}

/// This computer, in the shared folder.
pub fn machine_id(store: &Store) -> String {
    if let Some(known) = setting(store, MACHINE_KEY) {
        return known;
    }
    let id = uuid::Uuid::new_v4().to_string()[..8].to_owned();
    let _ = store.save_setting(MACHINE_KEY, &id);
    id
}

/// This computer's name in the other computers' files.
pub fn machine_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Mac"
    } else if cfg!(windows) {
        "Windows"
    } else {
        "Linux"
    }
}

/// The export platform of a computer's name, for the comparison (PLATFORMS).
pub fn platform_of(machine_name: &str) -> &'static str {
    match machine_name {
        "Mac" => "macos",
        "Windows" => "windows",
        "Linux" => "linux",
        _ => "",
    }
}

pub fn load_base(store: &Store) -> BTreeMap<String, String> {
    setting(store, BASE_KEY)
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_base(store: &Store, base: &BTreeMap<String, String>) -> Result<(), String> {
    store.save_setting(
        BASE_KEY,
        &serde_json::to_string(base).map_err(|e| e.to_string())?,
    )
}

/// The user chose for a bot both had changed: that version is now agreed
/// (settleBot).
pub fn settle(store: &Store, sync_id: &str, hash: &str) -> Result<(), String> {
    let mut base = load_base(store);
    base.insert(sync_id.into(), hash.into());
    save_base(store, &base)
}

pub fn load_status(store: &Store) -> Option<Status> {
    setting(store, LAST_KEY).and_then(|raw| serde_json::from_str(&raw).ok())
}

fn save_status(store: &Store, status: &Status) {
    if let Ok(raw) = serde_json::to_string(status) {
        let _ = store.save_setting(LAST_KEY, &raw);
    }
}

/// This computer's file name in the shared folder.
pub fn file_name(machine: &str) -> String {
    format!("{FILE_PREFIX}{machine}.json")
}

/// Every bot here as this computer's file carries it.
fn own_bots(
    store: &Store,
    root: &Path,
    roster: &Roster,
    travels: &mut Travels,
) -> Result<Vec<SyncedBot>, String> {
    transfer::exportable(roster)
        .map(|mono| {
            let model = transfer::session_model(store, mono);
            transfer::snapshot(root, mono, travels, model).map(|bot| SyncedBot::new(&bot))
        })
        .collect()
}

/// Writes this computer's file. Unchanged bots leave the file alone: the
/// cloud folder has nothing to upload. True when it wrote.
pub fn write_own_file(
    folder: &Path,
    machine: &str,
    bots: &[SyncedBot],
    now: u64,
) -> Result<bool, String> {
    let path = folder.join(file_name(machine));
    let fingerprint = |list: &[SyncedBot]| {
        let mut ids: Vec<String> = list
            .iter()
            .map(|bot| format!("{}:{}", bot.sync_id(), bot.hash))
            .collect();
        ids.sort();
        ids.join(",")
    };
    let before = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| SyncFile::parse(&raw));
    if before.is_some_and(|file| fingerprint(&file.bots) == fingerprint(bots)) {
        return Ok(false);
    }
    let file = SyncFile {
        format: FORMAT.into(),
        version: VERSION,
        machine: machine.into(),
        machine_name: machine_name().into(),
        updated_at: now,
        bots: bots.to_vec(),
    };
    std::fs::write(&path, file.to_text()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

/// The other computers' files in the folder; ones that don't read are skipped.
pub fn read_other_files(folder: &Path, machine: &str) -> Result<Vec<SyncFile>, String> {
    let own = file_name(machine);
    let entries = std::fs::read_dir(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let mut paths: Vec<_> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_dir()))
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with(FILE_PREFIX) && name.ends_with(".json") && name != own
        })
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    Ok(paths
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .filter_map(|raw| SyncFile::parse(&raw))
        .collect())
}

/// One sync: writes this computer's bots, takes what changed on the others,
/// and returns the bots both changed, for the user to decide (runBotSync).
/// The roster and the bots' travel state are saved when they change; how it
/// went is kept for [`load_status`].
pub fn run(store: &Store, root: &Path, roster: &mut Roster, now: u64) -> Vec<Conflict> {
    let Some(folder) = load_folder(store) else {
        return Vec::new();
    };
    let machine = machine_id(store);
    match run_in(store, root, roster, Path::new(&folder), &machine, now) {
        Ok((conflicts, status)) => {
            save_status(store, &status);
            conflicts
        }
        Err(error) => {
            save_status(
                store,
                &Status {
                    at: now,
                    text: error,
                    error: Some(true),
                },
            );
            Vec::new()
        }
    }
}

fn run_in(
    store: &Store,
    root: &Path,
    roster: &mut Roster,
    folder: &Path,
    machine: &str,
    now: u64,
) -> Result<(Vec<Conflict>, Status), String> {
    let mut travels = Travels::load(store);
    let mine = own_bots(store, root, roster, &mut travels)?;
    write_own_file(folder, machine, &mine, now)?;
    // Bots syncing for the first time were given their ids.
    travels.save(store)?;
    let others = read_other_files(folder, machine)?;
    let remote = newest_remote(&others, machine);
    let mut base = load_base(store);
    let local: Vec<LocalBot> = transfer::exportable(roster)
        .filter_map(|mono| {
            let sync_id = travels.get(&mono.id)?.sync_id.as_deref()?;
            let bot = mine.iter().find(|bot| bot.sync_id() == sync_id)?;
            Some(LocalBot {
                id: mono.id.clone(),
                sync_id: sync_id.into(),
                name: bot.bot.name.clone(),
                bot_role: bot.bot.bot_role.clone(),
                hash: bot.hash.clone(),
            })
        })
        .collect();
    let mut plan = plan(&local, &remote, &base);
    for adopt in &plan.adopt {
        travels.entry(&adopt.local_id).sync_id = Some(adopt.sync_id.clone());
    }
    for apply in &plan.apply {
        if roster.get(&apply.local_id).is_none() {
            continue;
        }
        transfer::replace(root, roster, &mut travels, &apply.local_id, &apply.bot.bot)?;
        plan.agreed
            .insert(apply.bot.sync_id().into(), apply.bot.hash.clone());
    }
    for bot in &plan.create {
        transfer::create_imported(root, roster, &mut travels, &bot.bot, false)?;
        plan.agreed.insert(bot.sync_id().into(), bot.hash.clone());
    }
    base.extend(plan.agreed.clone());
    save_base(store, &base)?;
    // What was taken from the others goes out again with this computer's file.
    if !plan.apply.is_empty() || !plan.create.is_empty() || !plan.adopt.is_empty() {
        roster.save(store)?;
        travels.save(store)?;
        let mine = own_bots(store, root, roster, &mut travels)?;
        write_own_file(folder, machine, &mine, now)?;
    }
    let mut names: Vec<&str> = Vec::new();
    for file in &others {
        if !names.contains(&file.machine_name.as_str()) {
            names.push(&file.machine_name);
        }
    }
    let changed = plan.apply.len() + plan.create.len();
    let text = if names.is_empty() {
        "Waiting for your other computer to sync to this folder".to_owned()
    } else if changed > 0 {
        format!(
            "Synced with {}: {changed} bot{} updated",
            names.join(", "),
            if changed == 1 { "" } else { "s" }
        )
    } else {
        format!("Synced with {}", names.join(", "))
    };
    Ok((
        plan.conflicts,
        Status {
            at: now,
            text,
            error: None,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mono::files;

    fn bot(patch: impl FnOnce(&mut Bot)) -> Bot {
        let mut bot = Bot {
            sync_id: Some("bbbbbbbb-1".into()),
            name: "Research".into(),
            mascot: "ghost".into(),
            color: "#7c3aed".into(),
            bot_role: Some("research".into()),
            soul: "# Research".into(),
            memory: "- Mining".into(),
            ..Default::default()
        };
        patch(&mut bot);
        bot
    }

    fn synced(patch: impl FnOnce(&mut Bot)) -> SyncedBot {
        let content = bot(patch);
        SyncedBot {
            hash: hash(&content),
            bot: content,
        }
    }

    fn local(patch: impl FnOnce(&mut Bot)) -> LocalBot {
        let content = bot(patch);
        LocalBot {
            id: "here".into(),
            sync_id: content.sync_id.clone().unwrap(),
            name: content.name.clone(),
            bot_role: content.bot_role.clone(),
            hash: hash(&content),
        }
    }

    fn remote(bots: Vec<SyncedBot>) -> Vec<Remote> {
        bots.into_iter()
            .map(|bot| Remote {
                bot,
                machine_name: "Mac".into(),
            })
            .collect()
    }

    fn base(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(id, hash)| (id.to_string(), hash.to_string()))
            .collect()
    }

    fn memory(text: &str) -> impl FnOnce(&mut Bot) + '_ {
        move |bot: &mut Bot| bot.memory = text.into()
    }

    #[test]
    fn ignores_a_habits_run_state_and_the_bots_identity() {
        let habit = r#""id":"h","name":"Papers","instructions":"x","schedule":{}"#;
        let before = bot(|b| b.habits = Some(format!(r#"[{{{habit},"nextRunAt":1,"runs":[]}}]"#)));
        let after = bot(|b| {
            b.sync_id = Some("zzzzzzzz-9".into());
            b.habits = Some(format!(
                r#"[{{{habit},"nextRunAt":99,"lastRunAt":50,"lastOutcome":"posted"}}]"#
            ));
        });
        assert_eq!(hash(&before), hash(&after));
        assert!(!content(&after).habits.unwrap().contains("lastRunAt"));
        assert_ne!(hash(&bot(memory("- Mining\n- Lima"))), hash(&before));
    }

    #[test]
    fn hashes_a_bot_as_the_desktop_does() {
        // botHash() of these bots, run from botSync.ts itself.
        assert_eq!(hash(&bot(|_| {})), "0e2dd4157770f4fe");
        let habits = r#"[{"id":"h","name":"Papers","schedule":{"time":"08:00","minute":0},"createdAt":1733400000000,"nextRunAt":5}]"#;
        let full = bot(|b| {
            b.specialty = Some("Reports — with sources".into());
            b.habits = Some(habits.into());
            b.topics = [("mining", "- Pozas"), ("2024", "- año")]
                .into_iter()
                .collect();
            b.bot_tools = Some(super::super::bot_tools::BotTools {
                connectors: Some(vec!["exa".into()]),
                ..Default::default()
            });
        });
        assert_eq!(hash(&full), "3567215d72c4392c");
        assert_eq!(
            content(&full).habits.as_deref(),
            Some("[\n  {\n    \"id\": \"h\",\n    \"name\": \"Papers\",\n    \"schedule\": {\n      \"time\": \"08:00\",\n      \"minute\": 0\n    },\n    \"createdAt\": 1733400000000\n  }\n]\n")
        );
    }

    #[test]
    fn reads_the_other_computers_files_newest_first() {
        let file = |machine: &str, updated_at: u64, memory_text: &str| SyncFile {
            format: FORMAT.into(),
            version: 1,
            machine: machine.into(),
            machine_name: machine.into(),
            updated_at,
            bots: vec![synced(memory(memory_text))],
        };
        let parsed = SyncFile::parse(&file("mac", 5, "new").to_text()).unwrap();
        assert_eq!(parsed.bots[0].bot.memory, "new");
        assert_eq!(parsed, file("mac", 5, "new"));
        assert!(SyncFile::parse("{}").is_none());
        let newest = newest_remote(
            &[file("me", 9, "mine"), file("old", 1, "old"), parsed],
            "me",
        );
        assert_eq!(newest.len(), 1);
        assert_eq!(newest[0].bot.bot.memory, "new");
        assert_eq!(newest[0].bot.sync_id(), "bbbbbbbb-1");
    }

    #[test]
    fn agrees_when_both_are_the_same() {
        let plan = plan(&[local(|_| {})], &remote(vec![synced(|_| {})]), &base(&[]));
        assert_eq!(plan.agreed, base(&[("bbbbbbbb-1", &synced(|_| {}).hash)]));
        assert!(plan.apply.is_empty());
    }

    #[test]
    fn takes_the_others_change_when_only_it_changed() {
        let plan = plan(
            &[local(|_| {})],
            &remote(vec![synced(memory("- Mining\n- Lima"))]),
            &base(&[("bbbbbbbb-1", &local(|_| {}).hash)]),
        );
        assert_eq!(plan.apply.len(), 1);
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn keeps_this_ones_change_for_the_other_to_take() {
        let plan = plan(
            &[local(memory("- changed here"))],
            &remote(vec![synced(|_| {})]),
            &base(&[("bbbbbbbb-1", &synced(|_| {}).hash)]),
        );
        assert!(plan.apply.is_empty());
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn asks_the_user_when_both_changed() {
        let plan = plan(
            &[local(memory("- here"))],
            &remote(vec![synced(memory("- there"))]),
            &base(&[("bbbbbbbb-1", &synced(|_| {}).hash)]),
        );
        assert_eq!(plan.conflicts.len(), 1);
        assert_eq!(plan.conflicts[0].local_id, "here");
        assert_eq!(plan.conflicts[0].machine_name, "Mac");
    }

    #[test]
    fn creates_a_new_bot_but_not_one_deleted_here() {
        let writer = synced(|b| {
            b.sync_id = Some("wwwwwwww-1".into());
            b.name = "Writer".into();
            b.bot_role = Some("writer".into());
        });
        let fresh = plan(&[], &remote(vec![writer.clone()]), &base(&[]));
        assert_eq!(fresh.create, std::slice::from_ref(&writer));
        let deleted = plan(
            &[],
            &remote(vec![writer.clone()]),
            &base(&[("wwwwwwww-1", &writer.hash)]),
        );
        assert!(deleted.create.is_empty());
    }

    #[test]
    fn pairs_the_two_computers_own_research_and_settles_on_one_id() {
        let here = local(|b| b.sync_id = Some("zzzzzzzz-9".into()));
        let there = synced(|b| b.sync_id = Some("aaaaaaaa-1".into()));
        let paired = plan(&[here], &remote(vec![there.clone()]), &base(&[]));
        assert_eq!(
            paired.adopt,
            [Adopt {
                local_id: "here".into(),
                sync_id: "aaaaaaaa-1".into()
            }]
        );
        assert_eq!(paired.agreed, base(&[("aaaaaaaa-1", &there.hash)]));
        // The other computer keeps its smaller id.
        let back = plan(
            &[local(|b| b.sync_id = Some("aaaaaaaa-1".into()))],
            &remote(vec![synced(|b| b.sync_id = Some("zzzzzzzz-9".into()))]),
            &base(&[]),
        );
        assert!(back.adopt.is_empty());
    }

    /// A store, an agent folder and a shared folder, synced to `folder`.
    struct Computer {
        _temp: tempfile::TempDir,
        store: Store,
        root: std::path::PathBuf,
        folder: std::path::PathBuf,
        roster: Roster,
    }

    impl Computer {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let store = Store::open(&temp.path().join("native.sqlite")).unwrap();
            let folder = temp.path().join("OneDrive").join("MonoBOT");
            std::fs::create_dir_all(&folder).unwrap();
            save_folder(&store, folder.to_str()).unwrap();
            Self {
                root: temp.path().join("monos"),
                folder,
                store,
                roster: Roster::default(),
                _temp: temp,
            }
        }
        /// The Research bot here, its id the one both computers share.
        fn research(&mut self, memory_text: &str) -> String {
            let id = self
                .roster
                .create_bot(&self.root, "research", false)
                .unwrap()
                .id
                .clone();
            let mut travels = Travels::load(&self.store);
            travels.entry(&id).sync_id = Some("aaaaaaaa-1".into());
            travels.save(&self.store).unwrap();
            self.write(&id, "SOUL.md", "# Research");
            self.write(&id, "MEMORY.md", memory_text);
            id
        }
        fn write(&self, id: &str, path: &str, text: &str) {
            files::save(&self.root, id, None, path, text, None).unwrap();
        }
        fn read(&self, id: &str, path: &str) -> Option<String> {
            files::read(&self.root, id, None, path).unwrap().text
        }
        /// The bot here as the Mac would have it, changed by `patch`.
        fn as_mac(&self, id: &str, patch: impl FnOnce(&mut Bot)) -> SyncedBot {
            let mut travels = Travels::load(&self.store);
            let mono = self.roster.get(id).unwrap().clone();
            let mut bot = transfer::snapshot(&self.root, &mono, &mut travels, None).unwrap();
            patch(&mut bot);
            SyncedBot {
                hash: hash(&bot),
                bot,
            }
        }
        fn mac_file(&self, bots: Vec<SyncedBot>) {
            let file = SyncFile {
                format: FORMAT.into(),
                version: 1,
                machine: "mac00001".into(),
                machine_name: "Mac".into(),
                updated_at: 1,
                bots,
            };
            std::fs::write(self.folder.join(file_name("mac00001")), file.to_text()).unwrap();
        }
        fn sync(&mut self, now: u64) -> Vec<Conflict> {
            run(&self.store, &self.root, &mut self.roster, now)
        }
        fn own_file(&self) -> SyncFile {
            let path = self.folder.join(file_name(&machine_id(&self.store)));
            SyncFile::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
        }
    }

    #[test]
    fn writes_this_computers_file_and_only_again_when_a_bot_changed() {
        let mut here = Computer::new();
        here.research("- Mining");
        assert!(here.sync(10).is_empty());
        let own = here.own_file();
        assert_eq!(own.bots[0].bot.name, "Research");
        assert_eq!(own.bots[0].sync_id(), "aaaaaaaa-1");
        assert_eq!(own.machine_name, machine_name());
        assert!(load_status(&here.store)
            .unwrap()
            .text
            .starts_with("Waiting for your other computer"));
        here.sync(20);
        assert_eq!(here.own_file().updated_at, 10);
    }

    #[test]
    fn takes_a_bot_that_changed_only_on_the_mac_and_creates_a_new_one() {
        let mut here = Computer::new();
        let id = here.research("- Mining");
        let agreed = here.as_mac(&id, |_| {});
        settle(&here.store, "aaaaaaaa-1", &agreed.hash).unwrap();
        let writer = here.as_mac(&id, |b| {
            b.sync_id = Some("wwwwwwww-1".into());
            b.name = "Writer".into();
            b.bot_role = Some("writer".into());
            b.memory = String::new();
            b.soul = "# Writer".into();
        });
        here.mac_file(vec![here.as_mac(&id, memory("- Mining\n- Lima")), writer]);
        assert!(here.sync(10).is_empty());
        assert_eq!(
            here.read(&id, "MEMORY.md").as_deref(),
            Some("- Mining\n- Lima")
        );
        let names: Vec<_> = here.roster.monos.iter().map(|m| m.label()).collect();
        assert_eq!(names, ["Research", "Writer"]);
        assert_eq!(
            load_status(&here.store).unwrap().text,
            "Synced with Mac: 2 bots updated"
        );
        // Both now agree, and the roster was saved with the new bot.
        assert_eq!(Roster::load(&here.store).unwrap().monos.len(), 2);
        let mine = here.own_file();
        let theirs = SyncFile::parse(
            &std::fs::read_to_string(here.folder.join(file_name("mac00001"))).unwrap(),
        )
        .unwrap();
        let hashes = |file: &SyncFile| {
            let mut list: Vec<_> = file.bots.iter().map(|b| b.hash.clone()).collect();
            list.sort();
            list
        };
        assert_eq!(hashes(&mine), hashes(&theirs));
    }

    #[test]
    fn asks_when_the_bot_changed_on_both_computers() {
        let mut here = Computer::new();
        let id = here.research("- Mining");
        settle(&here.store, "aaaaaaaa-1", &here.as_mac(&id, |_| {}).hash).unwrap();
        here.write(&id, "MEMORY.md", "- changed on Windows");
        here.mac_file(vec![here.as_mac(&id, memory("- changed on the Mac"))]);
        let conflicts = here.sync(10);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].local_id, id);
        assert_eq!(conflicts[0].machine_name, "Mac");
        assert_eq!(
            here.read(&id, "MEMORY.md").as_deref(),
            Some("- changed on Windows")
        );
    }

    #[test]
    fn says_why_a_run_failed_and_does_nothing_without_a_folder() {
        let mut here = Computer::new();
        here.research("- Mining");
        save_folder(&here.store, here.folder.join("missing").to_str()).unwrap();
        assert!(here.sync(10).is_empty());
        let status = load_status(&here.store).unwrap();
        assert_eq!(status.error, Some(true));
        assert_eq!(status.at, 10);
        save_folder(&here.store, None).unwrap();
        assert_eq!(load_folder(&here.store), None);
        assert!(here.sync(20).is_empty());
        assert_eq!(load_status(&here.store).unwrap().at, 10);
        let machine = machine_id(&here.store);
        assert_eq!(machine.len(), 8);
        assert_eq!(machine_id(&here.store), machine);
        assert_eq!(platform_of("Mac"), "macos");
    }
}
