//! The bots' News (botNews.ts, App.tsx): a bot that finished a turn while the
//! user was elsewhere, or posted a habit report, says so in the Principal's
//! News, and the Principal brings it up as a card. The feed is core's
//! (monocode_core::mono::news); this records into it and keeps the toasts.
use super::*;
use monocode_core::mono::{
    news::{self, Feed, Post, Toasts},
    outcomes::ReportStatus,
};
use monocode_core::time::now_ms;

#[derive(Default)]
pub(super) struct NewsState {
    feed: Option<Feed>,
    pub(super) toasts: Toasts,
    /// The bot whose news alone shows, from the filters.
    pub(super) only: Option<String>,
    /// Reports opened past their clamp.
    pub(super) expanded: std::collections::HashSet<String>,
    /// The report whose summary shows beside its header, on hover.
    pub(super) summary: super::hover_cards::Hover,
    /// Each report header's bounds, for its summary to open against.
    pub(super) anchors: HashMap<String, AnchorSlot>,
    /// The feed's scroll, for the fade that says there is more below.
    pub(super) scroll: gpui::ScrollHandle,
    /// Reports being saved as a note, and each one's last result.
    pub(super) note_pending: HashSet<String>,
    pub(super) note_feedback: HashMap<String, Result<(), String>>,
}

impl NewsState {
    /// The posts loaded so far; none until the feed is first read.
    pub(super) fn feed_items(&self) -> &[news::Item] {
        self.feed.as_ref().map_or(&[], |feed| &feed.items)
    }
}

impl Monos {
    pub(crate) fn news_feed(&mut self) -> &mut Feed {
        let store = self.store.clone();
        self.news
            .feed
            .get_or_insert_with(|| locked(&store).map(|db| Feed::load(&db)).unwrap_or_default())
    }

    fn save_news(&mut self, cx: &mut Context<Self>) {
        let feed = self.news_feed().clone();
        let store = self.store.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = locked(&store).and_then(|db| feed.save(&db));
            })
            .detach();
    }

    /// Adds a post; a repeat of a recent one stays out. Returns whether it
    /// was added, after queueing its card unless the bot's chat is open.
    fn post_news(&mut self, post: Post, cx: &mut Context<Self>) -> Option<news::Item> {
        let open = self.active.as_deref() == Some(post.mono_id.as_str());
        let added = self.news_feed().add(post, now_ms())?;
        if !open {
            self.news.toasts.queue(&added.id);
        }
        self.save_news(cx);
        self.changed(cx);
        Some(added)
    }

    /// A bot (not the Principal) finished a turn while the user was
    /// elsewhere: its reply goes to the News (botTurnNews).
    pub(super) fn bot_turn_news(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.active.as_deref() == Some(id) {
            return;
        }
        let Some(bot) = self
            .roster
            .get(id)
            .filter(|m| m.is_bot() && !m.is_principal())
            .cloned()
        else {
            return;
        };
        let Some(view) = self.chats.get(id) else {
            return;
        };
        let rows = view.read(cx).mono_rows(cx);
        let blocks: Vec<news::Block> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.first || row.role == "assistant")
            .map(|(index, row)| news::Block {
                id: index.to_string(),
                role: match row.role.as_str() {
                    "mono-system" => "user".into(),
                    role => role.into(),
                },
                text: row.text.clone(),
                internal: row.role == "mono-system",
                monocode: row.role == "mono-system",
                final_report: None,
            })
            .collect();
        let Some(turn) = news::turn_news(&blocks) else {
            return;
        };
        let session = view.read(cx).summary.id.to_string();
        self.post_news(
            Post {
                mono_id: bot.id.clone(),
                bot_name: bot.label(),
                source: turn.source,
                text: turn.text,
                at: None,
                status: Some(ReportStatus::Done),
                session_id: Some(session),
                block_id: Some(turn.block_id),
            },
            cx,
        );
    }

    /// A bot's habit report is news: routine posts alert at most once an
    /// hour, and ones that need the user alert at once.
    pub(super) fn bot_habit_news(
        &mut self,
        id: &str,
        habit: &str,
        text: &str,
        status: Option<ReportStatus>,
        cx: &mut Context<Self>,
    ) {
        let Some(bot) = self.roster.get(id).filter(|m| m.is_bot()).cloned() else {
            return;
        };
        let session = bot.session_id.map(|s| s.to_string());
        let added = self.post_news(
            Post {
                mono_id: bot.id.clone(),
                bot_name: bot.label(),
                source: habit.to_owned(),
                text: text.to_owned(),
                at: None,
                status,
                session_id: session,
                block_id: None,
            },
            cx,
        );
        if added.is_none() {
            return;
        }
        let alert = locked(&self.store)
            .map(|db| news::should_alert_stored(&db, status, now_ms()))
            .unwrap_or(false);
        if alert && let Some(session) = bot.session_id {
            cx.emit(NotificationPosted {
                session,
                body: format!("{}: {}", habit, news::preview(text, 240)),
            });
        }
    }

    /// "Save as note" on a report: the same note the transcript's saves,
    /// titled after the bot and where it posted (SaveNoteButton).
    pub(super) fn save_report_note(
        &mut self,
        item_id: &str,
        title: String,
        body: String,
        cx: &mut Context<Self>,
    ) {
        if !self.news.note_pending.insert(item_id.to_owned()) {
            return;
        }
        self.news.note_feedback.remove(item_id);
        let note = monocode_core::response::Note {
            id: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
            title: title
                .chars()
                .take(monocode_core::notes::MAX_TITLE)
                .collect(),
            body,
            tags: Vec::new(),
            source_session: 0,
            source_cwd: String::new(),
        };
        let store = self.store.clone();
        let work = cx
            .background_executor()
            .spawn(async move { locked(&store)?.save_note(&note) });
        let item_id = item_id.to_owned();
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| this.news_note_result(&item_id, result, cx));
        })
        .detach();
        cx.notify();
    }

    fn news_note_result(
        &mut self,
        item_id: &str,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        self.news.note_pending.remove(item_id);
        let success = result.is_ok();
        self.news.note_feedback.insert(item_id.to_owned(), result);
        cx.notify();
        if success {
            let item_id = item_id.to_owned();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let _ = this.update(cx, |this, cx| {
                    this.news.note_feedback.remove(&item_id);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// The Principal's overview opened: its News is read.
    pub(crate) fn mark_news_seen(&mut self, cx: &mut Context<Self>) {
        if self.news_feed().mark_seen() {
            self.save_news(cx);
            self.changed(cx);
        }
    }

    pub(crate) fn has_unseen_news(&mut self) -> bool {
        self.news_feed().has_unseen()
    }
}
