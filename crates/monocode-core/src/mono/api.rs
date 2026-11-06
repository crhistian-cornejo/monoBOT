use serde_json::Value;
/// Reject misspelled fields and ambiguous types before any app mutation.
pub fn validate(action: &str, input: &Value) -> Result<(), String> {
    let fields=match action{
        "models.list"|"soul.read"|"habits.list"=>"",
        "sessions.list"|"folders.list"|"worktrees.list"=>"project",
        "sessions.read"=>"sessionId project before limit maxChars",
        "sessions.start"=>"prompt project harness model modelSettings effort runtimeMode title draft reveal workspaceMode worktreeBase worktreeCwd placement besideSessionId notifyOnComplete",
        "sessions.send"=>"sessionId prompt project notifyOnComplete",
        "sessions.draft"=>"sessionId prompt project",
        "sessions.stop"|"sessions.archive"|"sessions.delete"=>"sessionId project",
        "worktrees.create"=>"branch base existing project",
        "folders.move"=>"sessionId folderId newFolderName project",
        "notes.list"=>"limit offset","notes.read"=>"id","notes.write"=>"id title body tags",
        "artifacts.list"=>"kind limit offset","artifacts.read"=>"id","artifacts.write"=>"id kind title body summary",
        "papers.find"=>"query limit fromYear","papers.read"=>"ref pages query section maxChars","papers.figure"=>"ref page","papers.add"=>"path ref title",
        "soul.update"=>"text expectedHash","memory.read"=>"topic","memory.search"=>"query since",
        "memory.add"=>"fact topic until","memory.replace"=>"find fact topic until","memory.remove"=>"find topic",
        "habits.add"=>"name instructions schedule","habits.update"=>"id name instructions schedule enabled","habits.run"|"habits.remove"=>"id",
        "chat.card"=>"type repo number note sessionId options name instructions schedule",
        "mail.list"=>"view query unread limit","mail.read"=>"id maxChars","mail.draft"=>"id text",
        "mail.search"=>"query account limit oldestFirst","mail.connect"=>"address provider name shared",
        // The agents' own actions (agentApp.ts): bots, tools, other agents, wakes and names.
        "bots.list"|"tools.read"|"agents.list"|"group.members"=>"",
        "bots.create"=>"name role specialty instructions services skills access connectors",
        "tools.update"=>"services skills access connectors",
        "agents.message"=>"to message wait","agents.wait"=>"waitId",
        "group.ask"=>"member message wait","group.access"=>"target reason group",
        "wakes.add"=>"in at note","name.update"=>"name",
        "web.read"=>"url query maxChars","web.download"=>"url name",
        "images.generate"=>"prompt count aspect source",
        _=>return Err("Unknown app action. Run app help".into()),
    };
    for (key, value) in input.as_object().ok_or("Input must be an object")? {
        if !fields.split_whitespace().any(|f| f == key) {
            return Err(format!("{action} does not take {key}"));
        }
        if [
            "draft",
            "reveal",
            "notifyOnComplete",
            "existing",
            "enabled",
            "unread",
            "oldestFirst",
            "shared",
            "wait",
        ]
        .contains(&key.as_str())
            && !value.is_boolean()
        {
            return Err(format!("{key} must be true or false"));
        }
        if ["services", "skills", "access", "connectors"].contains(&key.as_str())
            && !value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string))
        {
            return Err(format!("{key} must be an array"));
        }
        if key == "in" && !value.is_number() {
            return Err("in must be a number of minutes".into());
        }
        if [
            "limit", "offset", "maxChars", "number", "fromYear", "page", "count",
        ]
        .contains(&key.as_str())
            && value.as_u64().is_none()
        {
            return Err(format!("{key} must be a non-negative integer"));
        }
        if key == "before" && !value.is_string() && value.as_u64().is_none() {
            return Err("before must be a turn ID".into());
        }
        if matches!(key.as_str(), "id" | "sessionId" | "besideSessionId")
            && !value.is_string()
            && value.as_u64().is_none()
        {
            return Err(format!("{key} must be an ID"));
        }
        if ![
            "draft",
            "reveal",
            "notifyOnComplete",
            "existing",
            "enabled",
            "unread",
            "oldestFirst",
            "shared",
            "before",
            "limit",
            "offset",
            "maxChars",
            "number",
            "fromYear",
            "page",
            "id",
            "sessionId",
            "besideSessionId",
            "modelSettings",
            "schedule",
            "tags",
            "options",
            "wait",
            "services",
            "skills",
            "access",
            "connectors",
            "in",
            "count",
        ]
        .contains(&key.as_str())
            && !value.is_string()
        {
            return Err(format!("{key} must be text"));
        }
    }
    if input.get("modelSettings").is_some_and(|value| {
        value
            .as_object()
            .is_none_or(|values| values.values().any(|v| !v.is_string()))
    }) {
        return Err("modelSettings must be an object of strings".into());
    }
    if input["draft"] == true && input["notifyOnComplete"] == true {
        return Err("An unsent draft cannot send a completion notification".into());
    }
    if input.get("runtimeMode").is_some_and(|v| {
        !matches!(
            v.as_str(),
            Some("read-only" | "supervised" | "auto-accept-edits" | "auto" | "full-access")
        )
    }) {
        return Err("Unknown runtimeMode".into());
    }
    if input
        .get("workspaceMode")
        .is_some_and(|v| !matches!(v.as_str(), Some("current" | "checkout" | "worktree")))
    {
        return Err("workspaceMode must be current or worktree".into());
    }
    if input
        .get("placement")
        .is_some_and(|v| !matches!(v.as_str(), Some("tab" | "right" | "down")))
    {
        return Err("placement must be tab, right or down".into());
    }
    if input.get("besideSessionId").is_some()
        && !matches!(input["placement"].as_str(), Some("right" | "down"))
    {
        return Err("besideSessionId requires placement right or down".into());
    }
    if input.get("tags").is_some_and(|v| {
        v.as_array().is_none_or(|a| {
            a.len() > crate::notes::MAX_TAGS
                || a.iter().any(|v| {
                    v.as_str()
                        .is_none_or(|s| s.len() > crate::notes::MAX_TAG_LENGTH)
                })
        })
    }) {
        return Err("tags must be an array of up to 20 short strings".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn validates_before_mutation() {
        assert!(validate("sessions.start", &json!({"prompt":"x","draft":"false"})).is_err());
        assert!(validate("sessions.delete", &json!({"sessionId":1,"force":true})).is_err());
        assert!(validate("sessions.start", &json!({"runtimeMode":"unsafe"})).is_err());
        assert!(validate(
            "papers.find",
            &json!({"query":"GR4J","limit":5,"fromYear":2015})
        )
        .is_ok());
        assert!(validate("papers.find", &json!({"query":"GR4J","fromYear":"2015"})).is_err());
        assert!(validate(
            "papers.read",
            &json!({"ref":"10.1016/j.envsoft.2017.05.002","pages":"1-3"})
        )
        .is_ok());
        assert!(validate("papers.read", &json!({"ref":"x","download":true})).is_err());
        assert!(validate("papers.read", &json!({"ref":"x","section":"methods"})).is_ok());
        assert!(validate("papers.figure", &json!({"ref":"x","page":4})).is_ok());
        assert!(validate("papers.figure", &json!({"ref":"x","page":"4"})).is_err());
        assert!(validate(
            "mail.list",
            &json!({"view":"inbox","unread":true,"limit":20})
        )
        .is_ok());
        assert!(validate("mail.list", &json!({"unread":"yes"})).is_err());
        assert!(validate(
            "agents.message",
            &json!({"to":"Finance","message":"m","wait":true})
        )
        .is_ok());
        assert!(validate("agents.message", &json!({"to":"Finance","wait":"yes"})).is_err());
        assert!(validate("bots.create", &json!({"name":"S","services":["gmail"]})).is_ok());
        assert!(validate("bots.create", &json!({"name":"S","services":"gmail"})).is_err());
        assert!(validate("wakes.add", &json!({"in":30,"note":"check"})).is_ok());
        assert!(validate("wakes.add", &json!({"in":"30"})).is_err());
        assert!(validate("tools.update", &json!({"connectors":[]})).is_ok());
        assert!(validate("mail.draft", &json!({"id":"a.inbox.1","text":"Hola"})).is_ok());
        assert!(validate("mail.draft", &json!({"id":"a.inbox.1","to":"x@y.z"})).is_err());
        assert!(validate(
            "mail.search",
            &json!({"query":"from:bcp","oldestFirst":true})
        )
        .is_ok());
        assert!(validate("mail.connect", &json!({"address":"a@b.c","shared":"no"})).is_err());
        assert!(validate(
            "papers.add",
            &json!({"path":"/Users/me/paper.pdf","ref":"10.1234/x","title":"T"})
        )
        .is_ok());
        assert!(validate("papers.add", &json!({"path":"/p.pdf","url":"https://x"})).is_err());
        assert!(validate(
            "sessions.start",
            &json!({"prompt":"x","project":"/code","draft":true})
        )
        .is_ok());
    }
}
