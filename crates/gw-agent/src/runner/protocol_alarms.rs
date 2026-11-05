use super::*;
pub(super) fn evaluate_alarm_protocols(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
    points: &[(String, f64)],
) {
    let Ok(protocol_points) = ctx.store.alarm_protocol_points_for_meter(meter.record.id)
    else {
        return;
    };
    if protocol_points.is_empty() {
        return;
    }
    for rule in protocol_points {
        let event_id = format!(
            "ALARM_PROTOCOL:{}:{}:{}", rule.protocol_id, meter.record.device_sn, rule
            .point_code.to_lowercase()
        );
        let value = points
            .iter()
            .find(|(code, _)| code.eq_ignore_ascii_case(&rule.point_code))
            .map(|(_, value)| *value);
        let Some(value) = value.filter(|value| value.is_finite()) else {
            continue;
        };
        if protocol_point_triggered(&rule, value) {
            let threshold = protocol_threshold_text(&rule);
            let message = format!(
                "{} {}={} 触发告警条件 {}", meter.record.device_sn, rule
                .point_name, value, threshold
            );
            raise_protocol_alarm(ctx, meter, &event_id, &rule, value, message);
        } else if protocol_point_recovered(&rule, value) {
            recover_protocol_alarm(ctx, meter, &event_id, &rule, value);
        }
    }
}
pub(super) fn protocol_point_triggered(
    rule: &AlarmProtocolPointRecord,
    value: f64,
) -> bool {
    match rule.operator.trim().to_ascii_lowercase().as_str() {
        ">" | "gt" => rule.threshold_value.is_some_and(|limit| value > limit),
        ">=" | "gte" => rule.threshold_value.is_some_and(|limit| value >= limit),
        "<" | "lt" => rule.threshold_value.is_some_and(|limit| value < limit),
        "<=" | "lte" => rule.threshold_value.is_some_and(|limit| value <= limit),
        "=" | "==" | "eq" => {
            rule.threshold_value
                .is_some_and(|limit| (value - limit).abs() < f64::EPSILON)
        }
        "between" => {
            rule.threshold_min
                .zip(rule.threshold_max)
                .is_some_and(|(min, max)| value >= min && value <= max)
        }
        _ => false,
    }
}
pub(super) fn protocol_point_recovered(
    rule: &AlarmProtocolPointRecord,
    value: f64,
) -> bool {
    if let Some(recovery) = rule.recovery_threshold_value {
        return match rule.operator.trim().to_ascii_lowercase().as_str() {
            ">" | ">=" | "gt" | "gte" => value <= recovery,
            "<" | "<=" | "lt" | "lte" => value >= recovery,
            _ => !protocol_point_triggered(rule, value),
        };
    }
    !protocol_point_triggered(rule, value)
}
pub(super) fn protocol_threshold_text(rule: &AlarmProtocolPointRecord) -> String {
    if rule.operator.eq_ignore_ascii_case("between") {
        return format!(
            "between {} and {}", rule.threshold_min.unwrap_or_default(), rule
            .threshold_max.unwrap_or_default()
        );
    }
    format!("{} {}", rule.operator, rule.threshold_value.unwrap_or_default())
}
pub(super) fn protocol_threshold_value(rule: &AlarmProtocolPointRecord) -> Option<f64> {
    if rule.operator.eq_ignore_ascii_case("between") {
        return rule.threshold_max;
    }
    rule.threshold_value
}
pub(super) fn raise_protocol_alarm(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
    event_id: &str,
    rule: &AlarmProtocolPointRecord,
    value: f64,
    message: String,
) {
    let now = ctx.now_ms();
    let row = gw_store::AlarmEventRecord {
        source_event_id: event_id.to_string(),
        source: "GATEWAY".into(),
        meter_id: Some(meter.record.id),
        alarm_type: rule.alarm_type.clone(),
        level: rule.level.clone(),
        point_code: Some(rule.point_code.clone()),
        message: message.clone(),
        status: "ACTIVE".into(),
        first_seen_ms: now,
        last_seen_ms: now,
        cloud_status: "LOCAL".into(),
        cloud_message_id: None,
        cloud_ack_ms: None,
    };
    if matches!(ctx.store.upsert_alarm(& row), Ok(true)) {
        enqueue_gateway_alarm_with_context(
            ctx,
            meter,
            &row,
            "RAISED",
            format!("{message}，当前值 {value}"),
            Some(value),
            protocol_threshold_value(rule),
            Some(rule.point_name.clone()),
            Some(rule.operator.clone()),
        );
    }
}
pub(super) fn recover_protocol_alarm(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
    event_id: &str,
    rule: &AlarmProtocolPointRecord,
    value: f64,
) {
    if matches!(ctx.store.resolve_alarm("GATEWAY", event_id, ctx.now_ms()), Ok(true)) {
        let message = format!(
            "{} {} 已恢复，当前值 {}", meter.record.device_sn, rule.point_name,
            value
        );
        let row = gw_store::AlarmEventRecord {
            source_event_id: event_id.to_string(),
            source: "GATEWAY".into(),
            meter_id: Some(meter.record.id),
            alarm_type: rule.alarm_type.clone(),
            level: "INFO".into(),
            point_code: Some(rule.point_code.clone()),
            message: message.clone(),
            status: "RECOVERED".into(),
            first_seen_ms: ctx.now_ms(),
            last_seen_ms: ctx.now_ms(),
            cloud_status: "LOCAL".into(),
            cloud_message_id: None,
            cloud_ack_ms: None,
        };
        enqueue_gateway_alarm_with_context(
            ctx,
            meter,
            &row,
            "RECOVERED",
            message,
            Some(value),
            protocol_threshold_value(rule),
            Some(rule.point_name.clone()),
            Some(rule.operator.clone()),
        );
    }
}
