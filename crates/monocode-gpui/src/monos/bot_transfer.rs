//! Export and import of the bots, and what a sync needs of the roster
//! (botTransferIO.ts). The file, the comparison and the plan are core's
//! (mono::transfer); Settings and the import dialog only ask for them here.
use super::*;
use gpui::Task;
use monocode_core::mono::{
    sync,
    transfer::{self, Bot, Comparison, ImportPlan, Travels},
};
use monocode_core::time::now_ms;

/// A bot already here that an incoming one meets, read and compared.
#[derive(Clone)]
pub(crate) struct Meeting {
    pub current: Bot,
    pub comparison: Comparison,
}

/// An import sorted: new bots, and the ones that meet a bot here.
pub(crate) struct PreparedImport {
    pub plan: ImportPlan,
    /// One per conflict of the plan, in order.
    pub meetings: Vec<Option<Meeting>>,
}

impl Monos {
    /// Every bot as the file's text, and how many (exportBotsToFile).
    pub(crate) fn export_bots(
        &self,
        cx: &mut Context<Self>,
    ) -> Task<Result<(String, usize), String>> {
        let store = self.store.clone();
        let root = self.root.clone();
        let roster = self.roster.clone();
        cx.background_executor().spawn(async move {
            let db = locked(&store)?;
            let mut travels = Travels::load(&db);
            let out = transfer::export_text(&db, &root, &roster, &mut travels, now_ms())?;
            // Bots exported for the first time were given their sync ids.
            travels.save(&db)?;
            Ok(out)
        })
    }

    /// Sorts the incoming bots against the roster and reads the bots they
    /// meet. `matches` names the bot here each one is, when known (the sync
    /// pairs them by id).
    pub(crate) fn prepare_import(
        &self,
        bots: Vec<Bot>,
        matches: Option<Vec<Option<String>>>,
        cx: &mut Context<Self>,
    ) -> Task<Result<PreparedImport, String>> {
        let plan = transfer::plan_import(
            &bots,
            matches.as_deref(),
            &transfer::Existing::roster(&self.roster),
        );
        let store = self.store.clone();
        let root = self.root.clone();
        let roster = self.roster.clone();
        cx.background_executor().spawn(async move {
            let db = locked(&store)?;
            let mut travels = Travels::load(&db);
            let meetings = plan
                .conflicts
                .iter()
                .map(|conflict| {
                    let mono = roster.get(&conflict.existing_id)?;
                    let current = transfer::snapshot(
                        &root,
                        mono,
                        &mut travels,
                        transfer::session_model(&db, mono),
                    )
                    .ok()?;
                    let comparison = transfer::compare(&current, &conflict.incoming);
                    Some(Meeting {
                        current,
                        comparison,
                    })
                })
                .collect();
            travels.save(&db)?;
            Ok(PreparedImport { plan, meetings })
        })
    }

    /// Adds, replaces or copies the bots as the user chose, and says what it
    /// did (BotImportDialog's apply).
    pub(crate) fn apply_bot_import(
        &mut self,
        plan: &ImportPlan,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let db = locked(&self.store)?;
        let mut travels = Travels::load(&db);
        let summary = transfer::apply_import(&self.root, &mut self.roster, &mut travels, plan)?;
        travels.save(&db)?;
        drop(db);
        self.save(cx);
        Ok(summary)
    }

    /// The user chose for these bots both computers had changed: that
    /// version is now agreed, whatever was chosen (settleBot).
    pub(crate) fn settle_sync(&self, settled: &[sync::Conflict]) {
        if let Ok(db) = locked(&self.store) {
            for conflict in settled {
                let _ = sync::settle(&db, conflict.bot.sync_id(), &conflict.bot.hash);
            }
        }
    }
}
