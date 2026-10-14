//! Human recovery instructions depend on confirmed local publication, never backend acknowledgement.
use super::tr;
pub(super) fn notice(reason: &str, published: bool) -> String {
    let cause = match reason {
        "user_stop" | "execution_cancelled" => "Generation stopped. Start a new request from the current photo when you are ready.",
        "queue_full" | "capacity_full" => "The AI service is busy. Your request is kept; try submitting again later.",
        "photo_busy" => "The previous task is still finishing. Your request is kept; try submitting again later.",
        "context_full" => "The model conversation context is full. Delete this conversation before starting a new request.",
        "input_too_large" | "output_too_large" => {
            "The conversation exceeds service processing limits. Delete this conversation before starting a new request."
        }
        "capability_unsupported" => "This client does not support the proposed edit. Start a new request with a supported edit.",
        "invalid_plan" | "invalid_request" => {
            "The request or edit plan could not be validated. Adjust your request and start a new request from the current photo."
        }
        "ownership_denied" | "identity_failed" | "unknown_round" | "expired_request" | "client_gone" => {
            "The current AI task or connection is no longer valid. After it closes, start a new request."
        }
        "state_conflict" | "duplicate_conflict" | "execution_rejected_state" => {
            "The task state conflicted. A previous plan will not be replayed; start a new request from the current photo."
        }
        "round_timed_out" => "This AI round timed out. Start a new request from the current photo.",
        "delivery_timed_out" | "client_disconnected" | "backend_stopped" | "network_failure" => {
            "The AI connection was interrupted. Check the local service, then start a new request."
        }
        "model_failed" | "tool_failed" | "call_limit" => "The current AI stage could not finish. Start a new request from the current photo.",
        "execution_failed" => "This edit could not be completed. Start a new request from the current photo.",
        _ => "This AI task could not finish. Start a new request from the current photo.",
    };
    format!("{}\n{}", cause, if published { "The latest complete edit is kept." } else { "The photo remains at this round's starting settings." })
}
pub(super) fn published(record: &serde_json::Value) -> bool {
    record.get("stages").and_then(serde_json::Value::as_array).is_some_and(|stages| stages.iter().any(|s| s["execution_outcome"] == "succeeded"))
}
pub(super) fn trace(record: &serde_json::Value) -> Option<&str> {
    record.get("trace_id").and_then(serde_json::Value::as_str).filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
}

/// Only the displayed photo's captured, verified history can supply a cold diagnostic.
pub(super) fn diagnostic(conversation: &super::Conversation) -> Option<&str> {
    conversation.records.iter().rev().find_map(trace).or_else(|| conversation.view.as_ref().and_then(|view| view.diagnostic.as_deref()))
}

/// Keep authored human preflight causes; technical client errors stay in durable diagnostics.
pub(super) fn local_notice(error: &str, published: bool) -> String {
    for cause in [
        "The complete AI history exceeds the service limit. Local history is retained.",
        "The AI history is incomplete or invalid. Local history is retained. Repair attachments or delete the conversation to retry.",
        "AI history preparation failed. Try again.",
    ] {
        if error == cause || error == tr(cause) {
            return format!("{}\n{}", cause, notice("client_failure", published));
        }
    }
    notice(if error == "AI 方案与当前照片状态不匹配。" { "invalid_plan" } else { "client_failure" }, published)
}

/// Translate only app-owned system notices; authored/model messages use separate rows.
pub(super) fn display_notice(source: &str) -> String {
    source.lines().map(tr).collect::<Vec<_>>().join("\n")
}
