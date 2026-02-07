use chrono::{TimeZone, Utc};
use rusqlite::{params, Connection};
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::auth_manager;
use crate::types::{
    UsageBreakdownRow, UsageSummary, UsageTimeseriesPoint, VibeUsageDashboard,
};

#[derive(Debug, Clone, Copy)]
pub enum UsageRangeQuery {
    Last24Hours,
    Last7Days,
    Last30Days,
    AllTime,
}

impl UsageRangeQuery {
    pub fn from_input(input: &str) -> Self {
        match input.to_ascii_lowercase().as_str() {
            "24h" | "day" | "1d" => Self::Last24Hours,
            "7d" | "week" => Self::Last7Days,
            "30d" | "month" => Self::Last30Days,
            "all" | "all-time" | "all_time" => Self::AllTime,
            _ => Self::Last7Days,
        }
    }

    pub fn as_key(&self) -> &'static str {
        match self {
            Self::Last24Hours => "24h",
            Self::Last7Days => "7d",
            Self::Last30Days => "30d",
            Self::AllTime => "all",
        }
    }

    fn start_timestamp(&self, now_ts: i64) -> Option<i64> {
        match self {
            Self::Last24Hours => Some(now_ts - 24 * 60 * 60),
            Self::Last7Days => Some(now_ts - 7 * 24 * 60 * 60),
            Self::Last30Days => Some(now_ts - 30 * 24 * 60 * 60),
            Self::AllTime => None,
        }
    }

    fn bucket_sql(&self) -> &'static str {
        match self {
            Self::Last24Hours => "strftime('%Y-%m-%d %H:00:00', timestamp_utc, 'unixepoch')",
            Self::Last7Days | Self::Last30Days => {
                "strftime('%Y-%m-%d', timestamp_utc, 'unixepoch')"
            }
            Self::AllTime => "strftime('%Y-%m', timestamp_utc, 'unixepoch')",
        }
    }
}

#[derive(Debug, Clone)]
pub struct UsageEvent {
    pub request_id: String,
    pub timestamp_utc: i64,
    pub method: String,
    pub path: String,
    pub provider: String,
    pub model: String,
    pub account_key: String,
    pub account_label: String,
    pub status_code: i64,
    pub duration_ms: i64,
    pub request_bytes: i64,
    pub response_bytes: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub usage_json: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UsageTracker {
    db_path: PathBuf,
}

impl UsageTracker {
    pub fn new() -> Result<Self, String> {
        let db_path = auth_manager::get_auth_dir().join("vibeproxy-usage.db");
        let tracker = Self { db_path };
        tracker.init_schema()?;
        Ok(tracker)
    }

    fn open_connection(path: &Path) -> Result<Connection, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open usage database at {}: {}", path.display(), e))?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            "#,
        )
        .map_err(|e| format!("Failed to configure usage database: {}", e))?;
        Ok(conn)
    }

    fn init_schema(&self) -> Result<(), String> {
        let conn = Self::open_connection(&self.db_path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS usage_events (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              request_id TEXT NOT NULL,
              timestamp_utc INTEGER NOT NULL,
              day_utc TEXT NOT NULL,
              method TEXT NOT NULL,
              path TEXT NOT NULL,
              provider TEXT NOT NULL,
              model TEXT NOT NULL,
              account_key TEXT NOT NULL,
              account_label TEXT NOT NULL,
              status_code INTEGER NOT NULL,
              is_success INTEGER NOT NULL,
              duration_ms INTEGER NOT NULL,
              request_bytes INTEGER NOT NULL,
              response_bytes INTEGER NOT NULL,
              input_tokens INTEGER,
              output_tokens INTEGER,
              total_tokens INTEGER,
              cached_tokens INTEGER,
              reasoning_tokens INTEGER,
              usage_json TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_usage_events_timestamp
              ON usage_events(timestamp_utc);
            CREATE INDEX IF NOT EXISTS idx_usage_events_provider_model
              ON usage_events(provider, model);
            CREATE INDEX IF NOT EXISTS idx_usage_events_account
              ON usage_events(account_key);
            CREATE INDEX IF NOT EXISTS idx_usage_events_day
              ON usage_events(day_utc);

            CREATE TABLE IF NOT EXISTS usage_rollups_daily (
              day_utc TEXT NOT NULL,
              provider TEXT NOT NULL,
              model TEXT NOT NULL,
              account_key TEXT NOT NULL,
              requests INTEGER NOT NULL,
              total_tokens INTEGER NOT NULL,
              input_tokens INTEGER NOT NULL,
              output_tokens INTEGER NOT NULL,
              cached_tokens INTEGER NOT NULL DEFAULT 0,
              reasoning_tokens INTEGER NOT NULL DEFAULT 0,
              error_count INTEGER NOT NULL,
              PRIMARY KEY (day_utc, provider, model, account_key)
            );

            "#,
        )
        .map_err(|e| format!("Failed to initialize usage schema: {}", e))?;
        let _ = conn.execute(
            "ALTER TABLE usage_events ADD COLUMN cached_tokens INTEGER",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE usage_rollups_daily ADD COLUMN cached_tokens INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE usage_rollups_daily ADD COLUMN reasoning_tokens INTEGER NOT NULL DEFAULT 0",
            [],
        );
        self.backfill_usage_from_json(&conn)?;
        Ok(())
    }

    fn backfill_usage_from_json(&self, conn: &Connection) -> Result<(), String> {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, usage_json, cached_tokens, reasoning_tokens
                FROM usage_events
                WHERE usage_json IS NOT NULL
                  AND (cached_tokens IS NULL OR reasoning_tokens IS NULL)
                "#,
            )
            .map_err(|e| format!("Failed to prepare usage backfill query: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })
            .map_err(|e| format!("Failed to execute usage backfill query: {}", e))?;

        let mut updates: Vec<(i64, Option<i64>, Option<i64>)> = Vec::new();
        for row in rows {
            let (id, usage_json, cached_tokens, reasoning_tokens) =
                row.map_err(|e| format!("Failed to read usage backfill row: {}", e))?;

            let Some(raw) = usage_json else {
                continue;
            };
            let Ok(json) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };

            let extracted_cached = cached_tokens.or_else(|| {
                Self::find_number_in_json_deep(
                    &json,
                    &[
                        "cached_tokens",
                        "cached_input_tokens",
                        "cache_read_input_tokens",
                        "cache_creation_input_tokens",
                    ],
                )
            });
            let extracted_reasoning = reasoning_tokens.or_else(|| {
                Self::find_number_in_json_deep(
                    &json,
                    &["reasoning_tokens", "thinking_tokens", "reasoningTokenCount"],
                )
            });

            if extracted_cached != cached_tokens || extracted_reasoning != reasoning_tokens {
                updates.push((id, extracted_cached, extracted_reasoning));
            }
        }

        if !updates.is_empty() {
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| format!("Failed to start usage backfill transaction: {}", e))?;
            for (id, cached_tokens, reasoning_tokens) in updates {
                tx.execute(
                    "UPDATE usage_events SET cached_tokens = ?, reasoning_tokens = ? WHERE id = ?",
                    params![cached_tokens, reasoning_tokens, id],
                )
                .map_err(|e| format!("Failed to update usage backfill row {}: {}", id, e))?;
            }
            tx.commit()
                .map_err(|e| format!("Failed to commit usage backfill transaction: {}", e))?;
        }

        self.rebuild_daily_rollups(conn)
    }

    fn rebuild_daily_rollups(&self, conn: &Connection) -> Result<(), String> {
        conn.execute("DELETE FROM usage_rollups_daily", [])
            .map_err(|e| format!("Failed to clear daily rollups during rebuild: {}", e))?;
        conn.execute(
            r#"
            INSERT INTO usage_rollups_daily (
              day_utc, provider, model, account_key, requests,
              total_tokens, input_tokens, output_tokens, cached_tokens, reasoning_tokens, error_count
            )
            SELECT
              day_utc,
              provider,
              model,
              account_key,
              COUNT(*) AS requests,
              COALESCE(SUM(COALESCE(total_tokens, 0)), 0) AS total_tokens,
              COALESCE(SUM(COALESCE(input_tokens, 0)), 0) AS input_tokens,
              COALESCE(SUM(COALESCE(output_tokens, 0)), 0) AS output_tokens,
              COALESCE(SUM(COALESCE(cached_tokens, 0)), 0) AS cached_tokens,
              COALESCE(SUM(COALESCE(reasoning_tokens, 0)), 0) AS reasoning_tokens,
              COALESCE(SUM(CASE WHEN is_success = 0 THEN 1 ELSE 0 END), 0) AS error_count
            FROM usage_events
            GROUP BY day_utc, provider, model, account_key
            "#,
            [],
        )
        .map_err(|e| format!("Failed to rebuild daily rollups: {}", e))?;
        Ok(())
    }

    fn find_number_in_json_deep(value: &Value, keys: &[&str]) -> Option<i64> {
        match value {
            Value::Object(map) => {
                for key in keys {
                    if let Some(v) = map.get(*key) {
                        if let Some(n) = v.as_i64() {
                            return Some(n);
                        }
                        if let Some(n) = v.as_u64() {
                            return Some(n as i64);
                        }
                        if let Some(n) = v.as_f64() {
                            return Some(n.round() as i64);
                        }
                        if let Some(n) = v.as_str().and_then(|s| s.parse::<i64>().ok()) {
                            return Some(n);
                        }
                    }
                }
                for nested in map.values() {
                    if let Some(found) = Self::find_number_in_json_deep(nested, keys) {
                        return Some(found);
                    }
                }
                None
            }
            Value::Array(items) => {
                for nested in items {
                    if let Some(found) = Self::find_number_in_json_deep(nested, keys) {
                        return Some(found);
                    }
                }
                None
            }
            _ => None,
        }
    }

    pub async fn record_event(&self, event: UsageEvent) -> Result<(), String> {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || {
            let conn = Self::open_connection(&db_path)?;
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| format!("Failed to start usage transaction: {}", e))?;

            let day = Utc
                .timestamp_opt(event.timestamp_utc, 0)
                .single()
                .unwrap_or_else(Utc::now)
                .format("%Y-%m-%d")
                .to_string();
            let is_success = if (200..300).contains(&(event.status_code as u16)) {
                1_i64
            } else {
                0_i64
            };
            let total_tokens = event
                .total_tokens
                .or_else(|| match (event.input_tokens, event.output_tokens) {
                    (Some(input), Some(output)) => Some(input + output),
                    _ => None,
                });

            tx.execute(
                r#"
                INSERT INTO usage_events (
                  request_id, timestamp_utc, day_utc, method, path, provider, model,
                  account_key, account_label, status_code, is_success, duration_ms,
                  request_bytes, response_bytes, input_tokens, output_tokens,
                  total_tokens, cached_tokens, reasoning_tokens, usage_json
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
                params![
                    event.request_id,
                    event.timestamp_utc,
                    day,
                    event.method,
                    event.path,
                    event.provider,
                    event.model,
                    event.account_key,
                    event.account_label,
                    event.status_code,
                    is_success,
                    event.duration_ms,
                    event.request_bytes,
                    event.response_bytes,
                    event.input_tokens,
                    event.output_tokens,
                    total_tokens,
                    event.cached_tokens,
                    event.reasoning_tokens,
                    event.usage_json,
                ],
            )
            .map_err(|e| format!("Failed to insert usage event: {}", e))?;

            let error_count = if is_success == 1 { 0_i64 } else { 1_i64 };
            tx.execute(
                r#"
                INSERT INTO usage_rollups_daily (
                  day_utc, provider, model, account_key, requests, total_tokens,
                  input_tokens, output_tokens, cached_tokens, reasoning_tokens, error_count
                ) VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(day_utc, provider, model, account_key)
                DO UPDATE SET
                  requests = usage_rollups_daily.requests + 1,
                  total_tokens = usage_rollups_daily.total_tokens + excluded.total_tokens,
                  input_tokens = usage_rollups_daily.input_tokens + excluded.input_tokens,
                  output_tokens = usage_rollups_daily.output_tokens + excluded.output_tokens,
                  cached_tokens = usage_rollups_daily.cached_tokens + excluded.cached_tokens,
                  reasoning_tokens = usage_rollups_daily.reasoning_tokens + excluded.reasoning_tokens,
                  error_count = usage_rollups_daily.error_count + excluded.error_count
                "#,
                params![
                    day,
                    event.provider,
                    event.model,
                    event.account_key,
                    total_tokens.unwrap_or(0),
                    event.input_tokens.unwrap_or(0),
                    event.output_tokens.unwrap_or(0),
                    event.cached_tokens.unwrap_or(0),
                    event.reasoning_tokens.unwrap_or(0),
                    error_count,
                ],
            )
            .map_err(|e| format!("Failed to upsert daily usage rollup: {}", e))?;

            tx.commit()
                .map_err(|e| format!("Failed to commit usage transaction: {}", e))?;
            Ok(())
        })
        .await
        .map_err(|e| format!("Failed to join usage write task: {}", e))?
    }

    pub async fn get_vibe_dashboard(&self, range: UsageRangeQuery) -> Result<VibeUsageDashboard, String> {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || {
            let conn = Self::open_connection(&db_path)?;
            let now_ts = Utc::now().timestamp();
            let start_ts = range.start_timestamp(now_ts);

            let summary = if let Some(start) = start_ts {
                let mut stmt = conn
                    .prepare(
                        r#"
                        SELECT
                          COUNT(*),
                          COALESCE(SUM(COALESCE(total_tokens, 0)), 0),
                          COALESCE(SUM(COALESCE(input_tokens, 0)), 0),
                          COALESCE(SUM(COALESCE(output_tokens, 0)), 0),
                          COALESCE(SUM(COALESCE(cached_tokens, 0)), 0),
                          COALESCE(SUM(COALESCE(reasoning_tokens, 0)), 0),
                          COALESCE(SUM(CASE WHEN is_success = 0 THEN 1 ELSE 0 END), 0)
                        FROM usage_events
                        WHERE timestamp_utc >= ?
                        "#,
                    )
                    .map_err(|e| format!("Failed to prepare usage summary query: {}", e))?;
                stmt.query_row(params![start], |row| {
                    Ok(UsageSummary {
                        total_requests: row.get::<_, i64>(0)?,
                        total_tokens: row.get::<_, i64>(1)?,
                        input_tokens: row.get::<_, i64>(2)?,
                        output_tokens: row.get::<_, i64>(3)?,
                        cached_tokens: row.get::<_, i64>(4)?,
                        reasoning_tokens: row.get::<_, i64>(5)?,
                        error_count: row.get::<_, i64>(6)?,
                        error_rate: 0.0,
                    })
                })
                .map_err(|e| format!("Failed to execute usage summary query: {}", e))?
            } else {
                let mut stmt = conn
                    .prepare(
                        r#"
                        SELECT
                          COUNT(*),
                          COALESCE(SUM(COALESCE(total_tokens, 0)), 0),
                          COALESCE(SUM(COALESCE(input_tokens, 0)), 0),
                          COALESCE(SUM(COALESCE(output_tokens, 0)), 0),
                          COALESCE(SUM(COALESCE(cached_tokens, 0)), 0),
                          COALESCE(SUM(COALESCE(reasoning_tokens, 0)), 0),
                          COALESCE(SUM(CASE WHEN is_success = 0 THEN 1 ELSE 0 END), 0)
                        FROM usage_events
                        "#,
                    )
                    .map_err(|e| format!("Failed to prepare usage summary query: {}", e))?;
                stmt.query_row([], |row| {
                    Ok(UsageSummary {
                        total_requests: row.get::<_, i64>(0)?,
                        total_tokens: row.get::<_, i64>(1)?,
                        input_tokens: row.get::<_, i64>(2)?,
                        output_tokens: row.get::<_, i64>(3)?,
                        cached_tokens: row.get::<_, i64>(4)?,
                        reasoning_tokens: row.get::<_, i64>(5)?,
                        error_count: row.get::<_, i64>(6)?,
                        error_rate: 0.0,
                    })
                })
                .map_err(|e| format!("Failed to execute usage summary query: {}", e))?
            };

            let mut summary = summary;
            if summary.total_requests > 0 {
                summary.error_rate =
                    (summary.error_count as f64 / summary.total_requests as f64) * 100.0;
            }

            let bucket = range.bucket_sql();
            let timeseries_sql = if start_ts.is_some() {
                format!(
                    r#"
                    SELECT
                      {bucket} AS bucket,
                      COUNT(*) AS requests,
                      COALESCE(SUM(COALESCE(total_tokens, 0)), 0) AS total_tokens,
                      COALESCE(SUM(COALESCE(input_tokens, 0)), 0) AS input_tokens,
                      COALESCE(SUM(COALESCE(output_tokens, 0)), 0) AS output_tokens,
                      COALESCE(SUM(COALESCE(cached_tokens, 0)), 0) AS cached_tokens,
                      COALESCE(SUM(COALESCE(reasoning_tokens, 0)), 0) AS reasoning_tokens,
                      COALESCE(SUM(CASE WHEN is_success = 0 THEN 1 ELSE 0 END), 0) AS error_count
                    FROM usage_events
                    WHERE timestamp_utc >= ?
                    GROUP BY bucket
                    ORDER BY bucket ASC
                    "#
                )
            } else {
                format!(
                    r#"
                    SELECT
                      {bucket} AS bucket,
                      COUNT(*) AS requests,
                      COALESCE(SUM(COALESCE(total_tokens, 0)), 0) AS total_tokens,
                      COALESCE(SUM(COALESCE(input_tokens, 0)), 0) AS input_tokens,
                      COALESCE(SUM(COALESCE(output_tokens, 0)), 0) AS output_tokens,
                      COALESCE(SUM(COALESCE(cached_tokens, 0)), 0) AS cached_tokens,
                      COALESCE(SUM(COALESCE(reasoning_tokens, 0)), 0) AS reasoning_tokens,
                      COALESCE(SUM(CASE WHEN is_success = 0 THEN 1 ELSE 0 END), 0) AS error_count
                    FROM usage_events
                    GROUP BY bucket
                    ORDER BY bucket ASC
                    "#
                )
            };

            let mut stmt = conn
                .prepare(&timeseries_sql)
                .map_err(|e| format!("Failed to prepare timeseries query: {}", e))?;
            let mut rows = if let Some(start) = start_ts {
                stmt.query(params![start])
                    .map_err(|e| format!("Failed to query usage timeseries: {}", e))?
            } else {
                stmt.query([])
                    .map_err(|e| format!("Failed to query usage timeseries: {}", e))?
            };

            let mut timeseries: Vec<UsageTimeseriesPoint> = Vec::new();
            while let Some(row) = rows
                .next()
                .map_err(|e| format!("Failed to iterate usage timeseries rows: {}", e))?
            {
                timeseries.push(UsageTimeseriesPoint {
                    bucket: row.get::<_, String>(0).unwrap_or_else(|_| "".to_string()),
                    requests: row.get::<_, i64>(1).unwrap_or(0),
                    total_tokens: row.get::<_, i64>(2).unwrap_or(0),
                    input_tokens: row.get::<_, i64>(3).unwrap_or(0),
                    output_tokens: row.get::<_, i64>(4).unwrap_or(0),
                    cached_tokens: row.get::<_, i64>(5).unwrap_or(0),
                    reasoning_tokens: row.get::<_, i64>(6).unwrap_or(0),
                    error_count: row.get::<_, i64>(7).unwrap_or(0),
                });
            }

            let breakdown_sql = if start_ts.is_some() {
                r#"
                SELECT
                  provider,
                  model,
                  account_key,
                  account_label,
                  COUNT(*) AS requests,
                  COALESCE(SUM(COALESCE(total_tokens, 0)), 0) AS total_tokens,
                  COALESCE(SUM(COALESCE(input_tokens, 0)), 0) AS input_tokens,
                  COALESCE(SUM(COALESCE(output_tokens, 0)), 0) AS output_tokens,
                  COALESCE(SUM(COALESCE(cached_tokens, 0)), 0) AS cached_tokens,
                  COALESCE(SUM(COALESCE(reasoning_tokens, 0)), 0) AS reasoning_tokens,
                  COALESCE(SUM(CASE WHEN is_success = 0 THEN 1 ELSE 0 END), 0) AS error_count,
                  MAX(timestamp_utc) AS last_seen
                FROM usage_events
                WHERE timestamp_utc >= ?
                GROUP BY provider, model, account_key, account_label
                ORDER BY total_tokens DESC, requests DESC
                LIMIT 200
                "#
            } else {
                r#"
                SELECT
                  provider,
                  model,
                  account_key,
                  account_label,
                  COUNT(*) AS requests,
                  COALESCE(SUM(COALESCE(total_tokens, 0)), 0) AS total_tokens,
                  COALESCE(SUM(COALESCE(input_tokens, 0)), 0) AS input_tokens,
                  COALESCE(SUM(COALESCE(output_tokens, 0)), 0) AS output_tokens,
                  COALESCE(SUM(COALESCE(cached_tokens, 0)), 0) AS cached_tokens,
                  COALESCE(SUM(COALESCE(reasoning_tokens, 0)), 0) AS reasoning_tokens,
                  COALESCE(SUM(CASE WHEN is_success = 0 THEN 1 ELSE 0 END), 0) AS error_count,
                  MAX(timestamp_utc) AS last_seen
                FROM usage_events
                GROUP BY provider, model, account_key, account_label
                ORDER BY total_tokens DESC, requests DESC
                LIMIT 200
                "#
            };

            let mut stmt = conn
                .prepare(breakdown_sql)
                .map_err(|e| format!("Failed to prepare breakdown query: {}", e))?;
            let mut rows = if let Some(start) = start_ts {
                stmt.query(params![start])
                    .map_err(|e| format!("Failed to query usage breakdown: {}", e))?
            } else {
                stmt.query([])
                    .map_err(|e| format!("Failed to query usage breakdown: {}", e))?
            };

            let mut breakdown = Vec::new();
            while let Some(row) = rows
                .next()
                .map_err(|e| format!("Failed to iterate usage breakdown rows: {}", e))?
            {
                let last_seen_ts: i64 = row.get::<_, i64>(11).unwrap_or(0);
                let last_seen = if last_seen_ts > 0 {
                    Utc.timestamp_opt(last_seen_ts, 0)
                        .single()
                        .map(|dt| dt.to_rfc3339())
                } else {
                    None
                };
                breakdown.push(UsageBreakdownRow {
                    provider: row.get::<_, String>(0).unwrap_or_else(|_| "unknown".to_string()),
                    model: row.get::<_, String>(1).unwrap_or_else(|_| "unknown".to_string()),
                    account_key: row.get::<_, String>(2).unwrap_or_else(|_| "unknown".to_string()),
                    account_label: row.get::<_, String>(3).unwrap_or_else(|_| "unknown".to_string()),
                    requests: row.get::<_, i64>(4).unwrap_or(0),
                    total_tokens: row.get::<_, i64>(5).unwrap_or(0),
                    input_tokens: row.get::<_, i64>(6).unwrap_or(0),
                    output_tokens: row.get::<_, i64>(7).unwrap_or(0),
                    cached_tokens: row.get::<_, i64>(8).unwrap_or(0),
                    reasoning_tokens: row.get::<_, i64>(9).unwrap_or(0),
                    error_count: row.get::<_, i64>(10).unwrap_or(0),
                    last_seen,
                });
            }

            Ok(VibeUsageDashboard {
                range: range.as_key().to_string(),
                summary,
                timeseries,
                breakdown,
            })
        })
        .await
        .map_err(|e| format!("Failed to join usage dashboard query task: {}", e))?
    }

}
