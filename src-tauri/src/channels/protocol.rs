use crate::{error, state::AppState};
use deltamod_protocol_domain::{parse_deep_link, plan_response, PendingOpen};
use serde_json::{json, Value};

pub fn dispatch(state: &AppState, channel: &str, data: &[Value]) -> Result<Option<Value>, String> {
    match channel {
        "protocol:parseDeepLink" => {
            let raw = data
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| error::invalid("protocol:parseDeepLink"))?;
            let action =
                parse_deep_link(raw).map_err(|_| error::invalid("protocol:parseDeepLink"))?;
            Ok(Some(match action {
                deltamod_protocol_domain::CommunityAction::Launch { item_id } => {
                    json!({"kind":"launch","itemId":item_id})
                }
                deltamod_protocol_domain::CommunityAction::Import {
                    item_id,
                    file_id,
                    source,
                } => json!({"kind":"import","itemId":item_id,"fileId":file_id,"source":source}),
            }))
        }
        "protocol:planRange" => {
            let header = data.first().and_then(Value::as_str);
            let total = data
                .get(1)
                .and_then(Value::as_u64)
                .ok_or_else(|| error::invalid("protocol:planRange"))?;
            let plan =
                plan_response(header, total).map_err(|_| error::invalid("protocol:planRange"))?;
            Ok(Some(
                json!({"status":plan.status,"contentLength":plan.content_length,"contentRange":plan.content_range}),
            ))
        }
        "protocol:queueDeepLink" => {
            let raw = data
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| error::invalid("protocol:queueDeepLink"))?;
            parse_deep_link(raw).map_err(|_| error::invalid("protocol:queueDeepLink"))?;
            state
                .pending
                .push(PendingOpen::DeepLink(raw.to_owned()))
                .map_err(|_| error::invalid("protocol:queueDeepLink"))?;
            Ok(Some(Value::Null))
        }
        "protocol:rendererReady" => {
            let pending = state
                .pending
                .mark_renderer_ready()
                .map_err(|_| error::internal())?;
            let values = pending
                .into_iter()
                .map(|item| match item {
                    PendingOpen::DeepLink(value) => json!({"kind":"deepLink","value":value}),
                    PendingOpen::File(_) => Value::Null,
                })
                .filter(|value| !value.is_null())
                .take(256)
                .collect::<Vec<_>>();
            Ok(Some(json!(values)))
        }
        _ => Ok(None),
    }
}
