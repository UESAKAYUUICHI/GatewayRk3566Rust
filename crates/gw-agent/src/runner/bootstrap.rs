use super::*;
pub(super) fn import_profiles(store: &Store, bootstrap: &BootstrapConfig) {
    let dir = std::path::Path::new(&bootstrap.profiles_dir);
    let Ok(entries) = std::fs::read_dir(dir) else {
        tracing::info!("档案目录 {} 不存在，跳过导入", dir.display());
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                MeterProfile::from_toml_str(&text).map_err(|e| e.to_string())
            })
        {
            Ok(profile) => {
                let rows: Vec<gw_store::RegisterMapRow> = profile
                    .register
                    .iter()
                    .map(|spec| gw_store::RegisterMapRow {
                        profile: profile.name.clone(),
                        point_code: spec.point.clone(),
                        point_name: local_point_metadata(&spec.point).0.to_string(),
                        unit: local_point_metadata(&spec.point).1.to_string(),
                        func: spec.func,
                        address: spec.address,
                        quantity: spec.quantity,
                        field_offset: 0,
                        field_quantity: spec.quantity,
                        bit_offset: None,
                        bit_length: None,
                        data_type: spec.data_type.as_str().to_string(),
                        byte_order: spec.byte_order.trim().to_ascii_uppercase(),
                        scale: spec.scale,
                        offset: spec.offset,
                    })
                    .collect();
                match store.replace_register_map(&profile.name, &rows) {
                    Ok(()) => {
                        let _ = store
                            .register_local_thing_model(
                                &profile.name,
                                &profile.description,
                                0,
                            );
                        tracing::info!(
                            "已导入寄存器档案 {}（{} 点）", profile.name, rows
                            .len()
                        )
                    }
                    Err(e) => tracing::warn!("档案 {} 入库失败: {e}", profile.name),
                }
            }
            Err(e) => tracing::warn!("跳过非法档案 {}: {e}", path.display()),
        }
    }
}
pub(super) fn local_point_metadata(code: &str) -> (&'static str, &'static str) {
    match code {
        "voltage_a" => ("A相电压", "V"),
        "voltage_b" => ("B相电压", "V"),
        "voltage_c" => ("C相电压", "V"),
        "current_a" => ("A相电流", "A"),
        "current_b" => ("B相电流", "A"),
        "current_c" => ("C相电流", "A"),
        "active_power_total" => ("总有功功率", "kW"),
        "reactive_power_total" => ("总无功功率", "kvar"),
        "apparent_power_total" => ("总视在功率", "kVA"),
        "power_factor_total" => ("总功率因数", ""),
        "frequency" => ("频率", "Hz"),
        "forward_active_energy" => ("正向有功电能", "kWh"),
        _ => ("自定义测点", ""),
    }
}
pub(super) fn seed_meters(store: &Store, bootstrap: &BootstrapConfig, now_ms: u64) {
    for seed in &bootstrap.meter {
        if store.meter_by_sn(&seed.device_sn).unwrap_or(None).is_some() {
            continue;
        }
        let input = MeterInput {
            device_sn: seed.device_sn.clone(),
            device_name: if seed.device_name.is_empty() {
                seed.device_sn.clone()
            } else {
                seed.device_name.clone()
            },
            modbus_addr: seed.modbus_addr,
            profile: seed.profile.clone(),
            channel_id: seed.channel_id.clone(),
            upload_enabled: seed.upload_enabled,
            collect_interval_s: seed.collect_interval_s,
            enabled: seed.enabled,
        };
        match store.insert_meter(&input, now_ms) {
            Ok(id) => tracing::info!("种子电表 {} 入库 id={id}", seed.device_sn),
            Err(e) => tracing::warn!("种子电表 {} 入库失败: {e}", seed.device_sn),
        }
    }
}
