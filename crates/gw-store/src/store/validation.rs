use super::*;
pub(super) fn validate_register_row(row: &RegisterMapRow) -> StoreResult<()> {
    if row.point_code.trim().is_empty() {
        return Err(StoreError::Validation("point_code is required".into()));
    }
    if !matches!(row.func, 3 | 4) {
        return Err(
            StoreError::Validation(
                format!(
                    "point {} uses unsupported Modbus function code {}", row.point_code,
                    row.func
                ),
            ),
        );
    }
    let data_type = row.data_type.trim().to_ascii_lowercase();
    let expected_quantity = match data_type.as_str() {
        "u16" | "uint16" | "i16" | "int16" | "boolean" => 1,
        "u32" | "i32" | "f32" | "float32" => 2,
        _ => {
            return Err(
                StoreError::Validation(
                    format!(
                        "point {} uses unsupported data type {}", row.point_code, row
                        .data_type
                    ),
                ),
            );
        }
    };
    if row.field_quantity != expected_quantity {
        return Err(
            StoreError::Validation(
                format!(
                    "point {} quantity must be {} for {}", row.point_code,
                    expected_quantity, row.data_type
                ),
            ),
        );
    }
    let end = row.address as u32 + row.quantity as u32;
    if end > 65_536 {
        return Err(
            StoreError::Validation(
                format!(
                    "point {} register address overflows Modbus range", row.point_code
                ),
            ),
        );
    }
    let order = row.byte_order.trim().to_ascii_uppercase();
    let valid_order = match row.field_quantity {
        1 => matches!(order.as_str(), "" | "AB" | "BA" | "ABCD"),
        2 => matches!(order.as_str(), "" | "ABCD" | "BADC" | "CDAB" | "DCBA"),
        _ => false,
    };
    if !valid_order {
        return Err(
            StoreError::Validation(
                format!(
                    "point {} byte_order {} does not match quantity {}", row.point_code,
                    row.byte_order, row.field_quantity
                ),
            ),
        );
    }
    Ok(())
}
pub(super) fn normalize_parity(value: &str) -> StoreResult<String> {
    match value.trim().to_ascii_uppercase().as_str() {
        "N" | "NONE" => Ok("NONE".to_string()),
        "E" | "EVEN" => Ok("EVEN".to_string()),
        "O" | "ODD" => Ok("ODD".to_string()),
        other => Err(StoreError::Validation(format!("unsupported parity {other}"))),
    }
}
