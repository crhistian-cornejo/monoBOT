//! What personal bots post on their own (a habit's report, a digest, an
//! alert), gathered into one feed so the user reads the news in one place:
//! src/features/bots/model/botNews.ts, with the feed's filters, hover summary
//! and actions from src/features/bots/ui/BotNewsList.tsx.
//!
//! The feed is kept in the settings store under [`NEWS_KEY`] as the same JSON
//! array the desktop keeps in `monocode.botNews`; the first load takes the
//! desktop's feed when the native one has none yet.
use crate::store::Store;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

/// The same bot saying the same thing within a day is not news again.
pub const DUPLICATE_WINDOW_MS: u64 = 24 * 60 * 60 * 1000;
/// Routine posts alert at most this often; attention and blocked always do
/// (NEWS_ALERT_INTERVAL_MS).
pub const ALERT_INTERVAL_MS: u64 = 60 * 60 * 1000;
pub const MAX_ITEMS: usize = 200;
pub const MAX_TEXT: usize = 2_000;
/// How many news cards stack at once; older ones wait in the feed.
pub const MAX_TOASTS: usize = 3;
/// How long a report's hover summary may be (SUMMARY_MAX).
pub const SUMMARY_MAX: usize = 220;
/// The feed in the settings store (the desktop's `monocode.botNews`).
pub const NEWS_KEY: &str = "botNews";
/// When routine news last raised an alert (`monocode.botNews.lastAlert`).
pub const LAST_ALERT_KEY: &str = "botNews.lastAlert";
/// The desktop's own feed, read once when the native one is empty.
const DESKTOP_NEWS_KEY: &str = "monocode.botNews";

/// Where a habit's report stands (ReportStatus in monoHabits.ts).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Progress,
    Done,
    Blocked,
    NeedsAttention,
}

impl Status {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "progress" => Self::Progress,
            "done" => Self::Done,
            "blocked" => Self::Blocked,
            "needs_attention" => Self::NeedsAttention,
            _ => return None,
        })
    }
    /// The badge a report shows (STATUS_LABEL); progress shows none.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Progress => None,
            Self::Done => Some("Done"),
            Self::Blocked => Some("Blocked"),
            Self::NeedsAttention => Some("Needs you"),
        }
    }
}

/// One post in the feed (BotNewsItem).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub mono_id: String,
    pub bot_name: String,
    /// What posted it, e.g. the habit's name.
    pub source: String,
    pub text: String,
    pub at: u64,
    pub seen: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// The chat and the message it was posted as, to open it there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
}

/// Where a report was posted: its bot's chat and the message there (NewsPlace).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub session_id: String,
    pub block_id: String,
}

impl Item {
    /// Where "Open in chat" goes; none when it had no place saved.
    pub fn place(&self) -> Option<Place> {
        Some(Place {
            session_id: self.session_id.clone()?,
            block_id: self.block_id.clone()?,
        })
    }
    /// A report that needs the user asks for a reply.
    pub fn open_label(&self) -> &'static str {
        if self.status == Some(Status::NeedsAttention) {
            "Reply in chat"
        } else {
            "Open in chat"
        }
    }
    /// The title of the note "Save as note" makes of it.
    pub fn note_title(&self) -> String {
        format!("{} · {}", self.bot_name, self.source)
    }
    /// The short plain summary shown when its header is hovered.
    pub fn summary(&self) -> String {
        let text = preview(&self.text, SUMMARY_MAX);
        if text.is_empty() {
            "No text".into()
        } else {
            text
        }
    }
}

/// A post to add (addBotNews's argument); `at` defaults to now.
#[derive(Clone, Debug, Default)]
pub struct Post {
    pub mono_id: String,
    pub bot_name: String,
    pub source: String,
    pub text: String,
    pub at: Option<u64>,
    pub status: Option<Status>,
    pub session_id: Option<String>,
    pub block_id: Option<String>,
}

/// A bot that posted, with how many reports, for the filters (senders).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sender {
    pub mono_id: String,
    pub name: String,
    pub count: usize,
}

/// A filter chip's label: the name stays in the tooltip, the face and count show.
pub fn chip_label(name: &str, count: usize) -> String {
    format!("{name} · {count}")
}

/// The feed, newest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feed {
    pub items: Vec<Item>,
}

fn normalized(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A stored post, or none for anything else. A member's answer to a group was
/// once filed as news, and stays out.
fn stored_item(value: &Value) -> Option<Item> {
    let text = |key: &str| value.get(key).and_then(Value::as_str);
    let source = text("source").unwrap_or_default();
    if source.trim_start().starts_with("<group_message") {
        return None;
    }
    let at = value.get("at")?.as_f64()?;
    Some(Item {
        id: text("id")?.into(),
        mono_id: text("monoId")?.into(),
        bot_name: text("botName").unwrap_or_default().into(),
        source: source.into(),
        text: text("text")?.into(),
        at: at.max(0.) as u64,
        seen: value.get("seen").and_then(Value::as_bool).unwrap_or(false),
        status: text("status").and_then(Status::parse),
        session_id: text("sessionId").map(str::to_owned),
        block_id: text("blockId").map(str::to_owned),
    })
}

impl Feed {
    /// The stored feed, keeping only what reads as a post.
    pub fn parse(raw: &str) -> Self {
        let items = serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(stored_item)
            .collect();
        Self { items }
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.items).unwrap_or_else(|_| "[]".into())
    }
    /// The native feed, or the desktop's the first time.
    pub fn load(store: &Store) -> Self {
        let raw = store.setting(NEWS_KEY).ok().flatten().or_else(|| {
            store
                .desktop_preferences()
                .and_then(|preferences| preferences.get(DESKTOP_NEWS_KEY))
                .map(str::to_owned)
        });
        raw.map(|raw| Self::parse(&raw)).unwrap_or_default()
    }
    pub fn save(&self, store: &Store) -> Result<(), String> {
        store.save_setting(NEWS_KEY, &self.to_json())
    }

    /// Adds a post to the feed; a repeat of a recent one is dropped (none).
    pub fn add(&mut self, post: Post, now: u64) -> Option<Item> {
        let at = post.at.unwrap_or(now);
        let text: String = post.text.chars().take(MAX_TEXT).collect();
        let key = normalized(&text);
        let repeat = self.items.iter().any(|entry| {
            entry.mono_id == post.mono_id
                && at.saturating_sub(entry.at) < DUPLICATE_WINDOW_MS
                && normalized(&entry.text) == key
        });
        if repeat {
            return None;
        }
        let (session_id, block_id) = match (post.session_id, post.block_id) {
            (Some(session), Some(block)) if !session.is_empty() && !block.is_empty() => {
                (Some(session), Some(block))
            }
            _ => (None, None),
        };
        let entry = Item {
            id: uuid::Uuid::new_v4().to_string(),
            mono_id: post.mono_id,
            bot_name: post.bot_name,
            source: post.source,
            text,
            at,
            seen: false,
            status: post.status,
            session_id,
            block_id,
        };
        self.items.insert(0, entry.clone());
        self.items.truncate(MAX_ITEMS);
        Some(entry)
    }
    /// Reading the feed marks it seen; false when nothing changed.
    pub fn mark_seen(&mut self) -> bool {
        if self.items.iter().all(|item| item.seen) {
            return false;
        }
        for item in &mut self.items {
            item.seen = true;
        }
        true
    }
    /// A deleted bot's posts go with it.
    pub fn remove_for(&mut self, mono_id: &str) {
        self.items.retain(|item| item.mono_id != mono_id);
    }
    pub fn has_unseen(&self) -> bool {
        self.items.iter().any(|item| !item.seen)
    }
    pub fn get(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }
    /// The bots that posted, in the feed's order, with how many reports each.
    pub fn senders(&self) -> Vec<Sender> {
        let mut senders: Vec<Sender> = Vec::new();
        for item in &self.items {
            match senders.iter_mut().find(|s| s.mono_id == item.mono_id) {
                Some(known) => known.count += 1,
                None => senders.push(Sender {
                    mono_id: item.mono_id.clone(),
                    name: item.bot_name.clone(),
                    count: 1,
                }),
            }
        }
        senders
    }
    /// Filters show only when more than one bot posted.
    pub fn shows_filters(&self) -> bool {
        self.senders().len() > 1
    }
    /// The filter in effect: one whose bot has no news left shows everything.
    pub fn filter<'a>(&self, only: Option<&'a str>) -> Option<&'a str> {
        only.filter(|id| self.items.iter().any(|item| item.mono_id == *id))
    }
    /// The posts shown under a filter.
    pub fn shown(&self, only: Option<&str>) -> Vec<&Item> {
        let filter = self.filter(only);
        self.items
            .iter()
            .filter(|item| filter.is_none_or(|id| item.mono_id == id))
            .collect()
    }
}

/// Whether a post should raise an OS alert now. Attention and blocked always
/// do; the rest at most once an hour, the feed holding what came between.
/// `last` is when routine news last alerted, moved on when this one does.
pub fn should_alert(last: &mut u64, status: Option<Status>, now: u64) -> bool {
    if matches!(status, Some(Status::NeedsAttention | Status::Blocked)) {
        return true;
    }
    if now.saturating_sub(*last) < ALERT_INTERVAL_MS {
        return false;
    }
    *last = now;
    true
}

/// [`should_alert`] with the last alert kept in the settings store
/// (shouldAlertNews). Without storage it alerts.
pub fn should_alert_stored(store: &Store, status: Option<Status>, now: u64) -> bool {
    let mut last = store
        .setting(LAST_ALERT_KEY)
        .ok()
        .flatten()
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .map_or(0, |at| at.max(0.) as u64);
    let before = last;
    let alert = should_alert(&mut last, status, now);
    if last != before {
        let _ = store.save_setting(LAST_ALERT_KEY, &last.to_string());
    }
    alert
}

/// The posts shown as cards over the app, newest first. A card goes away once
/// dismissed, once the feed is read, or once the bot's posts are removed.
#[derive(Clone, Debug, Default)]
pub struct Toasts {
    ids: Vec<String>,
}

impl Toasts {
    /// Shows a post as a card (queueNewsToast); false when it already is one.
    pub fn queue(&mut self, id: &str) -> bool {
        if self.ids.iter().any(|known| known == id) {
            return false;
        }
        self.ids.insert(0, id.into());
        true
    }
    pub fn dismiss(&mut self, id: &str) -> bool {
        let before = self.ids.len();
        self.ids.retain(|known| known != id);
        self.ids.len() != before
    }
    /// The cards to show (newsToastsSnapshot).
    pub fn shown<'a>(&self, feed: &'a Feed) -> Vec<&'a Item> {
        self.ids
            .iter()
            .filter_map(|id| feed.get(id))
            .filter(|item| !item.seen)
            .take(MAX_TOASTS)
            .collect()
    }
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a valid pattern")
}

static FENCES: LazyLock<Regex> = LazyLock::new(|| regex(r"(?s)```.*?```"));
static IMAGES: LazyLock<Regex> = LazyLock::new(|| regex(r"!\[([^\]]*)\]\([^)]*\)"));
static LINKS: LazyLock<Regex> = LazyLock::new(|| regex(r"\[([^\]]+)\]\([^)]*\)"));
static LINE_MARKS: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?m)^\s{0,3}(#{1,6}|>|[-*+]|[0-9]+[.)])\s+"));
static PAIRED_MARKS: LazyLock<Regex> = LazyLock::new(|| regex(r"(\*\*|__|~~|`)"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| regex(r"\s+"));

/// JavaScript's `\w`.
fn word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `*emphasis*` and `_emphasis_` as their words, as
/// `/(^|[^\w*])[*_]([^*_\n]+)[*_](?=[^\w*]|$)/g` does.
fn drop_emphasis(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let boundary = |c: char| !word(c) && c != '*';
    // The emphasis opening at `open`: its words and where it ends.
    let emphasis = |open: usize| -> Option<(usize, usize)> {
        if !matches!(chars.get(open), Some('*' | '_')) {
            return None;
        }
        let end = (open + 1..chars.len())
            .find(|&i| matches!(chars[i], '*' | '_' | '\n'))
            .unwrap_or(chars.len());
        let closes = end > open + 1
            && matches!(chars.get(end), Some('*' | '_'))
            && chars.get(end + 1).is_none_or(|&c| boundary(c));
        closes.then_some((open + 1, end))
    };
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < chars.len() {
        // At the start the emphasis may open with no character before it.
        let mut found = None;
        if at == 0 {
            found = emphasis(0).map(|span| (None, span));
        }
        if found.is_none() && boundary(chars[at]) {
            found = emphasis(at + 1).map(|span| (Some(chars[at]), span));
        }
        match found {
            Some((prefix, (start, end))) => {
                out.extend(prefix);
                out.extend(&chars[start..end]);
                at = end + 1;
            }
            None => {
                out.push(chars[at]);
                at += 1;
            }
        }
    }
    out
}

/// A report's text as one plain line, for a banner or a card's preview: the
/// Markdown marks (bold, headings, list bullets, links, code) dropped and the
/// whitespace collapsed (newsPreview).
pub fn preview(text: &str, max: usize) -> String {
    let text = FENCES.replace_all(text, " ");
    let text = IMAGES.replace_all(&text, "$1");
    let text = LINKS.replace_all(&text, "$1");
    let text = LINE_MARKS.replace_all(&text, "");
    let text = PAIRED_MARKS.replace_all(&text, "");
    let text = drop_emphasis(&text);
    let plain = SPACES.replace_all(&text, " ").trim().to_owned();
    if plain.chars().count() > max {
        let cut: String = plain.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    } else {
        plain
    }
}

/// A message in a bot's chat, as a finished turn is read for the feed: the
/// parts of the desktop's Block that matter here.
#[derive(Clone, Debug, Default)]
pub struct Block {
    pub id: String,
    pub role: String,
    pub text: String,
    /// Written by the app, not the user.
    pub internal: bool,
    /// Sent by MonoBOT itself (Block.monocode).
    pub monocode: bool,
    /// The title of the final report the turn's request carries, if any.
    pub final_report: Option<String>,
}

/// A turn a group's chat asked of one of its members.
pub fn is_group_ask(block: &Block) -> bool {
    block.role == "user" && block.text.trim_start().starts_with("<group_message")
}

/// Whether the latest message a chat took was a group's.
pub fn answering_group(blocks: &[Block]) -> bool {
    blocks
        .iter()
        .rev()
        .find(|block| block.role == "user" && !block.internal)
        .is_some_and(is_group_ask)
}

static REACTION: LazyLock<Regex> =
    LazyLock::new(|| regex(r"^\s*<reaction>\s*([^<]{1,32}?)\s*</reaction>\s*$"));
static ONE_EMOJI: LazyLock<Regex> = LazyLock::new(|| {
    let picture = r"\p{Extended_Pictographic}(?:\x{FE0F}|\p{Emoji_Modifier})?";
    regex(&format!(
        r"^\s*(?:\p{{Regional_Indicator}}{{2}}|[#*0-9]\x{{FE0F}}?\x{{20E3}}|{picture}(?:\x{{200D}}{picture})*)\s*$"
    ))
});

/// How a reply reads outside the chat (monoReplyText): one that is only a
/// reaction says so.
pub fn reply_text(text: &str) -> String {
    match REACTION.captures(text).and_then(|found| found.get(1)) {
        Some(emoji) if ONE_EMOJI.is_match(emoji.as_str()) => {
            format!("Reacted {} to your message", emoji.as_str())
        }
        _ => text.into(),
    }
}

/// What a bot's finished turn tells the user, for the feed when they were
/// elsewhere (botTurnNews).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnNews {
    /// The request it answered (its first line), or "Finished".
    pub source: String,
    /// Its reply, under the final report it wrote, if any.
    pub text: String,
    /// The reply's message, where the post opens.
    pub block_id: String,
}

/// None when the turn left no reply, or answered a group, which shows it in
/// the group's chat.
pub fn turn_news(blocks: &[Block]) -> Option<TurnNews> {
    let start = blocks.iter().rposition(|block| block.role == "user")?;
    let request = &blocks[start];
    if is_group_ask(request) {
        return None;
    }
    let reply = blocks[start + 1..]
        .iter()
        .rev()
        .find(|block| block.role == "assistant" && !block.text.trim().is_empty())?;
    let text = reply_text(&reply.text).trim().to_owned();
    if text.is_empty() {
        return None;
    }
    let line = if request.internal || request.monocode {
        ""
    } else {
        request
            .text
            .split('\n')
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or_default()
    };
    let source = if line.is_empty() {
        "Finished".into()
    } else if line.chars().count() > 80 {
        let cut: String = line.chars().take(79).collect();
        format!("{}…", cut.trim_end())
    } else {
        line.into()
    };
    let text = match &request.final_report {
        Some(title) => format!("**Final report: {title}**\n\n{text}"),
        None => text,
    };
    Some(TurnNews {
        source,
        text,
        block_id: reply.id.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(mono: &str, bot: &str, source: &str, text: &str, at: Option<u64>) -> Post {
        Post {
            mono_id: mono.into(),
            bot_name: bot.into(),
            source: source.into(),
            text: text.into(),
            at,
            ..Default::default()
        }
    }

    fn block(id: &str, role: &str, text: &str) -> Block {
        Block {
            id: id.into(),
            role: role.into(),
            text: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn keeps_the_newest_post_first_and_unseen_until_read() {
        let mut feed = Feed::default();
        feed.add(post("a", "Finance", "Bank", "1", Some(1)), 0);
        feed.add(post("b", "Research", "Digest", "2", Some(2)), 0);
        let texts: Vec<_> = feed.items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, ["2", "1"]);
        assert!(feed.has_unseen());
        assert!(feed.mark_seen());
        assert!(!feed.has_unseen());
        assert!(!feed.mark_seen());
    }

    #[test]
    fn keeps_where_a_post_was_made_and_leaves_group_answers_out() {
        let mut feed = Feed::default();
        feed.add(
            Post {
                session_id: Some("s-chat".into()),
                block_id: Some("b1".into()),
                ..post("s", "Social Media", "Radar", "GPT-6", None)
            },
            5,
        );
        assert_eq!(
            feed.items[0].place(),
            Some(Place {
                session_id: "s-chat".into(),
                block_id: "b1".into()
            })
        );
        // A member's answer to a group is the group's, not news.
        assert_eq!(
            turn_news(&[
                block("u", "user", "<group_message group=\"Nexa\">\nHola"),
                block("a", "assistant", "Hola"),
            ]),
            None
        );
        // And one once filed as news drops out of the stored feed.
        let stored = Feed::parse(
            r#"[{"id":"1","monoId":"a","botName":"F","source":"<group_message group=\"N\">","text":"x","at":1,"seen":false},
                {"id":"2","monoId":"a","botName":"F","source":"Bank","text":"y","at":2,"seen":true,"status":"done"},
                {"id":"3","monoId":"a","text":"no time"}]"#,
        );
        assert_eq!(stored.items.len(), 1);
        assert_eq!(stored.items[0].status, Some(Status::Done));
        assert_eq!(Feed::parse(&stored.to_json()), stored);
    }

    #[test]
    fn keeps_a_repeat_of_a_recent_post_out_of_the_feed() {
        let mut feed = Feed::default();
        assert!(feed
            .add(post("a", "F", "Bank", "Same  thing", Some(1)), 0)
            .is_some());
        assert!(feed
            .add(post("a", "F", "Bank", "same thing", Some(2)), 0)
            .is_none());
        // Another bot may say it, and so may this one a day later.
        assert!(feed
            .add(post("b", "R", "Digest", "same thing", Some(3)), 0)
            .is_some());
        assert!(feed
            .add(
                post(
                    "a",
                    "F",
                    "Bank",
                    "same thing",
                    Some(1 + 25 * 60 * 60 * 1000)
                ),
                0
            )
            .is_some());
    }

    #[test]
    fn alerts_at_once_for_attention_otherwise_at_most_once_an_hour() {
        let now = 10 * ALERT_INTERVAL_MS;
        let mut last = 0;
        assert!(should_alert(&mut last, Some(Status::Progress), now));
        assert!(!should_alert(
            &mut last,
            Some(Status::Progress),
            now + 1_000
        ));
        assert!(should_alert(
            &mut last,
            Some(Status::NeedsAttention),
            now + 2_000
        ));
        assert!(should_alert(&mut last, Some(Status::Blocked), now + 3_000));
        assert!(should_alert(
            &mut last,
            Some(Status::Done),
            now + ALERT_INTERVAL_MS
        ));
    }

    #[test]
    fn keeps_the_feed_and_the_last_alert_in_the_settings_store() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        assert_eq!(Feed::load(&store), Feed::default());
        let mut feed = Feed::default();
        feed.add(post("a", "Finance", "Bank", "x", Some(1)), 0);
        feed.save(&store).unwrap();
        assert_eq!(Feed::load(&store), feed);
        let now = 10 * ALERT_INTERVAL_MS;
        assert!(should_alert_stored(&store, None, now));
        assert!(!should_alert_stored(&store, None, now + 1));
        assert_eq!(
            store.setting(LAST_ALERT_KEY).unwrap().as_deref(),
            Some(now.to_string().as_str())
        );
    }

    #[test]
    fn drops_a_deleted_bots_posts() {
        let mut feed = Feed::default();
        feed.add(post("a", "Finance", "Bank", "x", None), 1);
        feed.add(post("b", "Research", "Digest", "y", None), 2);
        feed.remove_for("a");
        let ids: Vec<_> = feed.items.iter().map(|i| i.mono_id.as_str()).collect();
        assert_eq!(ids, ["b"]);
    }

    #[test]
    fn previews_a_markdown_report_as_one_plain_line() {
        assert_eq!(
            preview(
                &[
                    "Lo más relevante:",
                    "",
                    "- **Kimi K3.1 Pro** — pesos en [HF](https://hf.co)",
                    "- `Hy4` *nuevo* MoE",
                ]
                .join("\n"),
                240
            ),
            "Lo más relevante: Kimi K3.1 Pro — pesos en HF Hy4 nuevo MoE"
        );
        assert_eq!(preview("## Title\n1. one", 240), "Title one");
        assert_eq!(preview(&"a".repeat(20), 10), format!("{}…", "a".repeat(9)));
        assert_eq!(
            preview("snake_case_name stays", 240),
            "snake_case_name stays"
        );
        assert_eq!(preview("_lead_ and *x*y", 240), "lead and *x*y");
        assert_eq!(preview("```\ncode\n``` ![pic](u) done", 240), "pic done");
    }

    #[test]
    fn shows_a_post_as_a_card_until_it_is_dismissed_or_read() {
        let mut feed = Feed::default();
        let first = feed.add(post("a", "F", "Bank", "1", None), 1).unwrap();
        let second = feed.add(post("b", "R", "Digest", "2", None), 2).unwrap();
        let mut toasts = Toasts::default();
        assert!(toasts.queue(&first.id));
        assert!(toasts.queue(&second.id));
        assert!(!toasts.queue(&second.id));
        let ids = |toasts: &Toasts, feed: &Feed| -> Vec<String> {
            toasts.shown(feed).iter().map(|i| i.id.clone()).collect()
        };
        assert_eq!(ids(&toasts, &feed), [second.id.clone(), first.id.clone()]);
        assert!(toasts.dismiss(&second.id));
        assert_eq!(ids(&toasts, &feed), std::slice::from_ref(&first.id));
        feed.mark_seen();
        assert!(ids(&toasts, &feed).is_empty());
    }

    #[test]
    fn tells_a_finished_turn_as_its_request_and_reply_with_the_final_report() {
        assert_eq!(
            turn_news(&[
                block("u0", "user", "old"),
                block("a0", "assistant", "old reply"),
                Block {
                    final_report: Some("Informe".into()),
                    ..block("u1", "user", "Pozas de paso para minería\nmás detalle")
                },
                block("a1", "assistant", "Primero busco."),
                block("a2", "assistant", "Listo, guardé el informe."),
            ]),
            Some(TurnNews {
                source: "Pozas de paso para minería".into(),
                text: "**Final report: Informe**\n\nListo, guardé el informe.".into(),
                block_id: "a2".into(),
            })
        );
        assert_eq!(
            turn_news(&[
                Block {
                    monocode: true,
                    ..block("u", "user", "hidden")
                },
                block("a", "assistant", "Done."),
            ]),
            Some(TurnNews {
                source: "Finished".into(),
                text: "Done.".into(),
                block_id: "a".into(),
            })
        );
        assert_eq!(turn_news(&[block("u", "user", "Hi")]), None);
        let long = "x".repeat(100);
        let news = turn_news(&[block("u", "user", &long), block("a", "assistant", "ok")]);
        assert_eq!(news.unwrap().source, format!("{}…", "x".repeat(79)));
    }

    #[test]
    fn reads_a_reaction_as_what_it_says() {
        assert_eq!(
            reply_text("<reaction>👍</reaction>"),
            "Reacted 👍 to your message"
        );
        assert_eq!(
            reply_text("<reaction>ok</reaction>"),
            "<reaction>ok</reaction>"
        );
        assert!(answering_group(&[
            block("u", "user", "<group_message group=\"N\">\nHi"),
            Block {
                internal: true,
                ..block("i", "user", "app note")
            },
        ]));
        assert!(!answering_group(&[block("u", "user", "Hi")]));
    }

    #[test]
    fn signs_and_filters_each_report_by_its_bot() {
        let mut feed = Feed::default();
        feed.add(
            Post {
                status: Some(Status::NeedsAttention),
                ..post("fin", "Finance", "Daily", "Two Netflix charges", Some(1))
            },
            0,
        );
        feed.add(
            Post {
                session_id: Some("social-chat".into()),
                block_id: Some("post-1".into()),
                ..post("social", "Social Media", "Radar", "GPT-6 Cyber", Some(2))
            },
            0,
        );
        // A report that needs the user asks for a reply; it had no place saved.
        let finance = &feed.items[1];
        assert_eq!(finance.open_label(), "Reply in chat");
        assert_eq!(finance.place(), None);
        assert_eq!(feed.items[0].open_label(), "Open in chat");
        assert_eq!(finance.note_title(), "Finance · Daily");
        assert_eq!(finance.summary(), "Two Netflix charges");
        assert_eq!(Status::NeedsAttention.label(), Some("Needs you"));
        assert_eq!(Status::Progress.label(), None);
        assert!(feed.shows_filters());
        let senders = feed.senders();
        assert_eq!(
            chip_label(&senders[1].name, senders[1].count),
            "Finance · 1"
        );
        assert_eq!(chip_label("All", feed.items.len()), "All · 2");
        let shown = feed.shown(Some("fin"));
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].text, "Two Netflix charges");
        // A filter whose bot has no news left shows everything again.
        feed.remove_for("fin");
        assert_eq!(feed.filter(Some("fin")), None);
        assert_eq!(feed.shown(Some("fin")).len(), 1);
        assert!(!feed.shows_filters());
    }
}
