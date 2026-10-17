//! Server-side knowledge results are history, never local executable requests.
use super::*;

pub(super) fn append_result(messages: &mut Vec<Value>, payload: &Value, round: &str, stage: &str) -> Result<(), String> {
    let invalid = || "AI 知识工具结果与当前调用不匹配。".to_owned();
    let id = payload.get("tool_call_id").and_then(Value::as_str).filter(|v| !v.is_empty() && v.len() <= 256).ok_or_else(invalid)?;
    let name = payload.get("tool_name").and_then(Value::as_str).ok_or_else(invalid)?;
    let text = payload.get("text").and_then(Value::as_str).filter(|v| v.len() <= 64 * 1024).ok_or_else(invalid)?;
    let model_call = payload.get("model_call_id").and_then(Value::as_u64).ok_or_else(invalid)?;
    if !matches!(name, "skill" | "read_skill_resource") || !matches!(stage, "initial" | "review") {
        return Err(invalid());
    }
    let owner = messages.iter().rposition(|m| m["role"] == "assistant").ok_or_else(invalid)?;
    let message = messages.get(owner).ok_or_else(invalid)?;
    if message["local_round_id"] != round || message["stage"] != stage || message["model_call_id"] != model_call {
        return Err(invalid());
    }
    let matched = message.get("tool_calls").and_then(Value::as_array).is_some_and(|calls| calls.iter().any(|c| c["id"] == id && c["name"] == name));
    if !matched {
        return Err(invalid());
    }
    if let Some(previous) = messages.iter().find(|m| m["role"] == "tool" && m["tool_call_id"] == id) {
        return if previous["text"] == text { Ok(()) } else { Err(invalid()) };
    }
    messages.push(json!({"role":"tool","text":text,"tool_call_id":id,"local_round_id":round,"stage":stage,
        "model_call_id":model_call,"complete_state":"complete","display_kind":"tool_result","local_message_id":format!("{round}-knowledge-{id}")}));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn history() -> Vec<Value> {
        vec![json!({"role":"assistant","text":"查看摄影资料。","local_round_id":"local-1","stage":"initial","model_call_id":1,
            "tool_calls":[{"id":"scoped-call","name":"skill","arguments":{"skill":"exposure-reading"}}]})]
    }
    fn payload() -> Value {
        json!({"tool_call_id":"scoped-call","tool_name":"skill","text":"资料正文","model_call_id":1})
    }
    #[test]
    fn server_knowledge_survives_archive_and_next_round_projection() {
        let mut messages = history();
        append_result(&mut messages, &payload(), "local-1", "initial").expect("valid server result");
        append_result(&mut messages, &payload(), "local-1", "initial").expect("idempotent duplicate");
        assert_eq!(messages.len(), 2);
        let saved = serde_json::to_vec(&messages).expect("archive");
        let loaded: Vec<Value> = serde_json::from_slice(&saved).expect("reload");
        let mut projection = projection::HistoryProjection::new(PhotoId(1));
        for item in &loaded {
            projection.push(item, |_| Err("no image expected".into())).expect("valid history");
        }
        let next = projection.finish().expect("matched calls");
        assert_eq!(next[1]["text"], "资料正文");
        assert_eq!(next[1]["tool_call_id"], "scoped-call");
    }
    #[test]
    fn server_results_reject_foreign_calls_stages_rounds_and_client_tools() {
        for (field, value) in [("tool_call_id", json!("unknown")), ("tool_name", json!("get_photo_state")), ("model_call_id", json!(2))] {
            let mut p = payload();
            p[field] = value;
            assert!(append_result(&mut history(), &p, "local-1", "initial").is_err());
        }
        assert!(append_result(&mut history(), &payload(), "other", "initial").is_err());
        assert!(append_result(&mut history(), &payload(), "local-1", "review").is_err());
        let mut p = payload();
        p["text"] = json!("x".repeat(64 * 1024 + 1));
        assert!(append_result(&mut history(), &p, "local-1", "initial").is_err());
    }
    #[test]
    fn stopped_knowledge_call_is_closed_without_inventing_success() {
        let mut messages = history();
        close_pending_tools(&mut messages, "cancelled");
        assert!(messages[1]["text"].as_str().expect("result").contains("cancelled"));
        assert!(append_result(&mut messages, &payload(), "local-1", "initial").is_err());
    }
}
