use super::*;
pub(super) fn build_meter_list(
    current: &Snapshot,
    store: &Store,
) -> Result<Vec<MeterDto>, ApiError> {
    let records = store.meters().map_err(|e| ApiError::internal(e.to_string()))?;
    let mut dtos = current
        .meters
        .iter()
        .map(|meter| meter_dto(meter, store))
        .collect::<Vec<_>>();
    for record in records {
        if dtos.iter().any(|meter| meter.sn == record.device_sn) {
            continue;
        }
        dtos.push(meter_dto_from_record(&record, store));
    }
    dtos.sort_by(|a, b| {
        let a_staged = a.channel_id == "__staging__";
        let b_staged = b.channel_id == "__staging__";
        a_staged
            .cmp(&b_staged)
            .then_with(|| a.channel_id.cmp(&b.channel_id))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(dtos)
}
pub(super) fn meter_dto(m: &MeterSnapshot, store: &Store) -> MeterDto {
    let point = |key: &str| {
        m.points
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| *value)
            .unwrap_or(0.0)
    };
    let record = store.meter_by_sn(&m.device_sn).ok().flatten();
    let mappings = store.register_map(&m.profile).unwrap_or_default();
    let mut points = mappings
        .iter()
        .map(|row| {
            let value = m
                .points
                .iter()
                .find(|(code, _)| code == &row.point_code)
                .map(|(_, value)| *value);
            PointDto {
                code: row.point_code.clone(),
                name: if row.point_name.is_empty() {
                    row.point_code.clone()
                } else {
                    row.point_name.clone()
                },
                value,
                unit: if row.unit.is_empty() {
                    default_unit(&row.point_code).to_string()
                } else {
                    row.unit.clone()
                },
                quality: if value.is_some() { m.quality } else { 0 },
                collect_time: if value.is_some() { m.last_read_ms } else { 0 },
            }
        })
        .collect::<Vec<_>>();
    for (code, value) in &m.points {
        if points.iter().any(|point| point.code == *code) {
            continue;
        }
        points
            .push(PointDto {
                code: code.clone(),
                name: code.clone(),
                value: Some(*value),
                unit: default_unit(code).to_string(),
                quality: m.quality,
                collect_time: m.last_read_ms,
            });
    }
    MeterDto {
        id: m.id,
        sn: m.device_sn.clone(),
        name: record
            .as_ref()
            .map(|r| r.device_name.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| m.device_sn.clone()),
        profile: m.profile.clone(),
        model_version: record
            .as_ref()
            .map(|r| r.model_version.clone())
            .unwrap_or_default(),
        channel_id: record
            .as_ref()
            .map(|r| r.channel_id.clone())
            .unwrap_or_else(default_channel_id),
        config_source: record
            .as_ref()
            .map(|r| r.config_source.clone())
            .unwrap_or_else(|| "LOCAL".into()),
        upload_enabled: record.as_ref().map(|r| r.upload_enabled).unwrap_or(false),
        enabled: record.as_ref().map(|r| r.enabled).unwrap_or(false),
        collect_interval_s: record.as_ref().map(|r| r.collect_interval_s).unwrap_or(0),
        continuous_pull: m.continuous_pull,
        address: m.addr,
        online: m.online,
        voltage: point("voltage_a"),
        current: point("current_a"),
        power: point("active_power_total"),
        energy: point("forward_active_energy"),
        last_read: format_time(m.last_read_ms),
        points,
    }
}
pub(super) fn meter_dto_from_record(record: &MeterRecord, store: &Store) -> MeterDto {
    let mappings = store.register_map(&record.profile).unwrap_or_default();
    let points = mappings
        .into_iter()
        .map(|row| PointDto {
            code: row.point_code.clone(),
            name: if row.point_name.is_empty() {
                row.point_code.clone()
            } else {
                row.point_name
            },
            value: None,
            unit: if row.unit.is_empty() {
                default_unit(&row.point_code).to_string()
            } else {
                row.unit
            },
            quality: 0,
            collect_time: 0,
        })
        .collect();
    MeterDto {
        id: record.id,
        sn: record.device_sn.clone(),
        name: if record.device_name.is_empty() {
            record.device_sn.clone()
        } else {
            record.device_name.clone()
        },
        profile: record.profile.clone(),
        model_version: record.model_version.clone(),
        channel_id: record.channel_id.clone(),
        config_source: record.config_source.clone(),
        upload_enabled: record.upload_enabled,
        enabled: record.enabled,
        collect_interval_s: record.collect_interval_s,
        continuous_pull: false,
        address: record.modbus_addr,
        online: false,
        voltage: 0.0,
        current: 0.0,
        power: 0.0,
        energy: 0.0,
        last_read: "--".into(),
        points,
    }
}
