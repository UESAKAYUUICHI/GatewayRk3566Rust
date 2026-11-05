use super::*;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ThingModelDto {
    pub(super) profile: String,
    pub(super) name: String,
    pub(super) version: String,
    pub(super) source: String,
    pub(super) point_count: u32,
    pub(super) device_count: u32,
    pub(super) points: Vec<ThingPointDto>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ThingPointDto {
    pub(super) code: String,
    pub(super) name: String,
    pub(super) unit: String,
    pub(super) function_code: u8,
    pub(super) address: u16,
    pub(super) quantity: u16,
    pub(super) field_offset: u16,
    pub(super) field_quantity: u16,
    pub(super) bit_offset: Option<u8>,
    pub(super) bit_length: Option<u8>,
    pub(super) data_type: String,
    pub(super) byte_order: String,
    pub(super) scale: f64,
    pub(super) offset: f64,
}
pub(super) async fn thing_models(
    State(state): State<WebState>,
) -> ApiResult<Vec<ThingModelDto>> {
    let summaries = state
        .store
        .thing_models()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(
        Json(
            summaries
                .into_iter()
                .map(|model| {
                    let points = state
                        .store
                        .register_map(&model.profile)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|p| ThingPointDto {
                            code: p.point_code,
                            name: p.point_name,
                            unit: p.unit,
                            function_code: p.func,
                            address: p.address,
                            quantity: p.quantity,
                            field_offset: p.field_offset,
                            field_quantity: p.field_quantity,
                            bit_offset: p.bit_offset,
                            bit_length: p.bit_length,
                            data_type: p.data_type,
                            byte_order: p.byte_order,
                            scale: p.scale,
                            offset: p.offset,
                        })
                        .collect();
                    ThingModelDto {
                        profile: model.profile,
                        name: model.name,
                        version: model.version,
                        source: model.source,
                        point_count: model.point_count,
                        device_count: model.device_count,
                        points,
                    }
                })
                .collect(),
        ),
    )
}
pub(super) async fn delete_thing_model(
    Path(profile): Path<String>,
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    state
        .store
        .delete_thing_model(&profile)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(Accepted { accepted: true }))
}
