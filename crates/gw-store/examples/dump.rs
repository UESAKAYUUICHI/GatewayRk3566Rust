//! 冒烟/联调辅助：打印网关数据库的 outbox/读数/事件概况。
//! 用法：cargo run -p gw-store --example dump -- [db路径]

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "gateway.db".to_string());
    let conn = rusqlite::Connection::open(&path).expect("打开数据库失败");
    let q = |sql: &str| -> Vec<String> {
        let mut stmt = conn.prepare(sql).expect(sql);
        let cols = stmt.column_count();
        let rows = stmt
            .query_map([], |r| {
                Ok((0..cols)
                    .map(|i| {
                        r.get::<_, Option<String>>(i)
                            .ok()
                            .flatten()
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join(" | "))
            })
            .expect(sql);
        rows.filter_map(|r| r.ok()).collect()
    };

    println!("== outbox 状态统计 ==");
    for r in q(
        "SELECT status, COUNT(*), MIN(created_ms), MAX(created_ms) FROM outbox_message GROUP BY status",
    ) {
        println!("  {r}");
    }
    println!(
        "首条: {:?}",
        q("SELECT message_id, topic, status FROM outbox_message ORDER BY id LIMIT 1").pop()
    );
    println!(
        "末条: {:?}",
        q("SELECT message_id, topic, status FROM outbox_message ORDER BY id DESC LIMIT 1").pop()
    );
    println!("== 读数快照 ==");
    for r in q("SELECT meter_id, read_ms, snapshot FROM meter_reading") {
        println!("  {r}");
    }
    println!("== 今日电能 ==");
    for r in q("SELECT meter_id, day, delta_kwh FROM energy_daily ORDER BY day") {
        println!("  {r}");
    }
    println!("== 事件（最早 8 条） ==");
    for r in q("SELECT ts_ms, level, source, message FROM event_log ORDER BY id LIMIT 8") {
        println!("  {r}");
    }
}
