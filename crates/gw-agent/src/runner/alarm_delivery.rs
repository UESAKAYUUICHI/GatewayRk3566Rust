use super::*;
pub(super) fn raise_gateway_alarm(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
    event_id: &str,
    alarm_type: &str,
    level: &str,
    message: String,
) {
    let now = ctx.now_ms();
    let row = gw_store::AlarmEventRecord {
        source_event_id: event_id.to_string(),
        source: "GATEWAY".into(),
        meter_id: Some(meter.record.id),
        alarm_type: alarm_type.into(),
        level: level.into(),
        point_code: None,
        message: message.clone(),
        status: "ACTIVE".into(),
        first_seen_ms: now,
        last_seen_ms: now,
        cloud_status: "LOCAL".into(),
        cloud_message_id: None,
        cloud_ack_ms: None,
    };
    if matches!(ctx.store.upsert_alarm(& row), Ok(true)) {
        enqueue_gateway_alarm(ctx, meter, &row, "RAISED", message);
    }
}
pub(super) fn recover_gateway_alarm(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
    event_id: &str,
    alarm_type: &str,
    message: String,
) {
    if matches!(ctx.store.resolve_alarm("GATEWAY", event_id, ctx.now_ms()), Ok(true)) {
        let row = gw_store::AlarmEventRecord {
            source_event_id: event_id.to_string(),
            source: "GATEWAY".into(),
            meter_id: Some(meter.record.id),
            alarm_type: alarm_type.into(),
            level: "INFO".into(),
            point_code: None,
            message: message.clone(),
            status: "RECOVERED".into(),
            first_seen_ms: ctx.now_ms(),
            last_seen_ms: ctx.now_ms(),
            cloud_status: "LOCAL".into(),
            cloud_message_id: None,
            cloud_ack_ms: None,
        };
        enqueue_gateway_alarm(ctx, meter, &row, "RECOVERED", message);
    }
}
pub(super) fn enqueue_gateway_alarm(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
    alarm: &gw_store::AlarmEventRecord,
    action: &str,
    message: String,
) {
    enqueue_gateway_alarm_with_context(
        ctx,
        meter,
        alarm,
        action,
        message,
        None,
        None,
        None,
        None,
    );
}
pub(super) fn enqueue_gateway_alarm_with_context(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
    alarm: &gw_store::AlarmEventRecord,
    action: &str,
    message: String,
    alarm_value: Option<f64>,
    threshold_value: Option<f64>,
    point_name: Option<String>,
    compare_operator: Option<String>,
) {
    let timestamp = ctx.state.next_message_ts(ctx.now_ms());
    let gateway_id = ctx.state.gateway_id();
    let message_id = proto::ids::alarm_message_id(&gateway_id, timestamp);
    let payload = proto::GatewayAlarmPayload {
        schema_version: proto::model::SCHEMA_VERSION.into(),
        message_id: message_id.clone(),
        gateway_sn: ctx.state.gateway_sn(),
        timestamp,
        kind: proto::model::TYPE_ALARM_UPLOAD.into(),
        event_id: alarm.source_event_id.clone(),
        action: action.into(),
        alarm_type: alarm.alarm_type.clone(),
        level: alarm.level.clone(),
        device_sn: Some(meter.record.device_sn.clone()),
        point_code: alarm.point_code.clone(),
        point_name,
        alarm_value,
        threshold_value,
        compare_operator,
        message,
    };
    let Ok(payload) = serde_json::to_vec(&payload) else {
        return;
    };
    let item = gw_store::NewOutbox {
        message_id: message_id.clone(),
        topic: topics::alarm_upload_topic(&gateway_id),
        payload,
        created_ms: timestamp,
    };
    if matches!(ctx.store.enqueue(& item), Ok(true)) {
        let _ = ctx.store.mark_alarm_cloud_pending(&alarm.source_event_id, &message_id);
        ctx.notify_publish.notify_one();
    }
}
