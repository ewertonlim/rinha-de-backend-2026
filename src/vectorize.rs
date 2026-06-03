use crate::models::{FraudRequest, NormalizationConfig};
use std::collections::HashMap;

/// Transforms a fraud request payload into a 14-dimensional normalized vector.
///
/// Each dimension is clamped to [0.0, 1.0] except indices 5 and 6 which use
/// the sentinel value -1.0 when `last_transaction` is null.
pub fn vectorize(
    req: &FraudRequest,
    norm: &NormalizationConfig,
    mcc_risk: &HashMap<String, f32>,
) -> [f32; 14] {
    let mut v = [0.0f32; 14];

    // 0: amount
    v[0] = clamp(req.transaction.amount / norm.max_amount);

    // 1: installments
    v[1] = clamp(req.transaction.installments as f64 / norm.max_installments);

    // 2: amount_vs_avg
    v[2] = clamp(
        (req.transaction.amount / req.customer.avg_amount) / norm.amount_vs_avg_ratio,
    );

    // 3: hour_of_day (UTC, 0-23)
    let hour = parse_hour(&req.transaction.requested_at);
    v[3] = (hour as f32) / 23.0;

    // 4: day_of_week (Mon=0, Sun=6)
    let dow = parse_day_of_week(&req.transaction.requested_at);
    v[4] = (dow as f32) / 6.0;

    // 5: minutes_since_last_tx
    // 6: km_from_last_tx
    match &req.last_transaction {
        Some(last) => {
            let minutes = compute_minutes_diff(
                &req.transaction.requested_at,
                &last.timestamp,
            );
            v[5] = clamp(minutes / norm.max_minutes);
            v[6] = clamp(last.km_from_current / norm.max_km);
        }
        None => {
            v[5] = -1.0;
            v[6] = -1.0;
        }
    }

    // 7: km_from_home
    v[7] = clamp(req.terminal.km_from_home / norm.max_km);

    // 8: tx_count_24h
    v[8] = clamp(req.customer.tx_count_24h as f64 / norm.max_tx_count_24h);

    // 9: is_online
    v[9] = if req.terminal.is_online { 1.0 } else { 0.0 };

    // 10: card_present
    v[10] = if req.terminal.card_present { 1.0 } else { 0.0 };

    // 11: unknown_merchant (1 = merchant NOT in known_merchants)
    v[11] = if req.customer.known_merchants.contains(&req.merchant.id) {
        0.0
    } else {
        1.0
    };

    // 12: mcc_risk (default 0.5 if MCC not found)
    v[12] = *mcc_risk.get(&req.merchant.mcc).unwrap_or(&0.5);

    // 13: merchant_avg_amount
    v[13] = clamp(req.merchant.avg_amount / norm.max_merchant_avg_amount);

    v
}

/// Clamp a value to [0.0, 1.0], converting from f64 to f32.
#[inline(always)]
fn clamp(x: f64) -> f32 {
    x.clamp(0.0, 1.0) as f32
}

/// Parse the hour (0-23 UTC) from an ISO 8601 timestamp string.
/// Expected format: "2026-03-11T18:45:53Z"
#[inline]
fn parse_hour(ts: &str) -> u32 {
    // The hour is at positions 11-12 in "YYYY-MM-DDTHH:MM:SSZ"
    ts[11..13].parse::<u32>().unwrap_or(0)
}

/// Parse day of week from ISO 8601 timestamp. Monday=0, Sunday=6.
/// Uses Tomohiko Sakamoto's algorithm to avoid chrono dependency for this.
#[inline]
fn parse_day_of_week(ts: &str) -> u32 {
    let y: i32 = ts[0..4].parse().unwrap_or(2026);
    let m: u32 = ts[5..7].parse().unwrap_or(1);
    let d: u32 = ts[8..10].parse().unwrap_or(1);

    // Tomohiko Sakamoto's algorithm — returns 0=Sunday, 1=Monday, ..., 6=Saturday
    let t = [0i32, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut yy = y;
    if m < 3 {
        yy -= 1;
    }
    let dow_sunday_0 =
        ((yy + yy / 4 - yy / 100 + yy / 400 + t[(m - 1) as usize] + d as i32) % 7) as u32;

    // Convert from Sunday=0 to Monday=0: (dow_sunday_0 + 6) % 7
    (dow_sunday_0 + 6) % 7
}

/// Compute the difference in minutes between two ISO 8601 timestamps.
/// Returns the absolute difference as f64.
#[inline]
fn compute_minutes_diff(current: &str, previous: &str) -> f64 {
    let current_secs = parse_timestamp_to_epoch(current);
    let previous_secs = parse_timestamp_to_epoch(previous);
    let diff_secs = (current_secs - previous_secs).unsigned_abs();
    diff_secs as f64 / 60.0
}

/// Parse "YYYY-MM-DDTHH:MM:SSZ" to seconds since a reference epoch.
/// We don't need real Unix epoch, just consistent relative timestamps.
#[inline]
fn parse_timestamp_to_epoch(ts: &str) -> i64 {
    let y: i64 = ts[0..4].parse().unwrap_or(0);
    let m: u32 = ts[5..7].parse().unwrap_or(1);
    let d: u32 = ts[8..10].parse().unwrap_or(1);
    let h: u32 = ts[11..13].parse().unwrap_or(0);
    let min: u32 = ts[14..16].parse().unwrap_or(0);
    let s: u32 = ts[17..19].parse().unwrap_or(0);

    // Days from year 0 (simplified — good enough for dates around 2026)
    let mut days: i64 = 365 * y + y / 4 - y / 100 + y / 400;

    // Month days (cumulative, non-leap year base)
    let month_days: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    days += month_days[(m - 1) as usize] as i64;

    // Leap year correction for months after February
    if m > 2 && (y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)) {
        days += 1;
    }

    days += d as i64;

    days * 86400 + h as i64 * 3600 + min as i64 * 60 + s as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn test_norm() -> NormalizationConfig {
        NormalizationConfig {
            max_amount: 10000.0,
            max_installments: 12.0,
            amount_vs_avg_ratio: 10.0,
            max_minutes: 1440.0,
            max_km: 1000.0,
            max_tx_count_24h: 20.0,
            max_merchant_avg_amount: 10000.0,
        }
    }

    fn test_mcc_risk() -> HashMap<String, f32> {
        let mut m = HashMap::new();
        m.insert("5411".to_string(), 0.15);
        m.insert("5812".to_string(), 0.30);
        m.insert("5912".to_string(), 0.20);
        m.insert("5944".to_string(), 0.45);
        m.insert("7801".to_string(), 0.80);
        m.insert("7802".to_string(), 0.75);
        m.insert("7995".to_string(), 0.85);
        m.insert("4511".to_string(), 0.35);
        m.insert("5311".to_string(), 0.25);
        m.insert("5999".to_string(), 0.50);
        m
    }

    #[test]
    fn test_legit_transaction_vectorization() {
        // Example from REGRAS_DE_DETECCAO.md — legit transaction
        // Expected: [0.0041, 0.1667, 0.05, 0.7826, 0.3333, -1, -1, 0.0292, 0.15, 0, 1, 0, 0.15, 0.006]
        let req = FraudRequest {
            id: "tx-1329056812".to_string(),
            transaction: Transaction {
                amount: 41.12,
                installments: 2,
                requested_at: "2026-03-11T18:45:53Z".to_string(),
            },
            customer: Customer {
                avg_amount: 82.24,
                tx_count_24h: 3,
                known_merchants: vec!["MERC-003".to_string(), "MERC-016".to_string()],
            },
            merchant: Merchant {
                id: "MERC-016".to_string(),
                mcc: "5411".to_string(),
                avg_amount: 60.25,
            },
            terminal: Terminal {
                is_online: false,
                card_present: true,
                km_from_home: 29.23,
            },
            last_transaction: None,
        };

        let v = vectorize(&req, &test_norm(), &test_mcc_risk());

        let expected = [
            0.0041f32, 0.1667, 0.05, 0.7826, 0.3333, -1.0, -1.0, 0.0292, 0.15, 0.0, 1.0, 0.0,
            0.15, 0.006,
        ];
        for i in 0..14 {
            assert!(
                (v[i] - expected[i]).abs() < 0.01,
                "dim {} mismatch: got {}, expected {}",
                i,
                v[i],
                expected[i]
            );
        }
    }

    #[test]
    fn test_fraud_transaction_vectorization() {
        // Example from REGRAS_DE_DETECCAO.md — fraudulent transaction
        // Expected: [0.9506, 0.8333, 1.0, 0.2174, 0.8333, -1, -1, 0.9523, 1.0, 0, 1, 1, 0.75, 0.0055]
        let req = FraudRequest {
            id: "tx-3330991687".to_string(),
            transaction: Transaction {
                amount: 9505.97,
                installments: 10,
                requested_at: "2026-03-14T05:15:12Z".to_string(),
            },
            customer: Customer {
                avg_amount: 81.28,
                tx_count_24h: 20,
                known_merchants: vec![
                    "MERC-008".to_string(),
                    "MERC-007".to_string(),
                    "MERC-005".to_string(),
                ],
            },
            merchant: Merchant {
                id: "MERC-068".to_string(),
                mcc: "7802".to_string(),
                avg_amount: 54.86,
            },
            terminal: Terminal {
                is_online: false,
                card_present: true,
                km_from_home: 952.27,
            },
            last_transaction: None,
        };

        let v = vectorize(&req, &test_norm(), &test_mcc_risk());

        let expected = [
            0.9506f32, 0.8333, 1.0, 0.2174, 0.8333, -1.0, -1.0, 0.9523, 1.0, 0.0, 1.0, 1.0,
            0.75, 0.0055,
        ];
        for i in 0..14 {
            assert!(
                (v[i] - expected[i]).abs() < 0.01,
                "dim {} mismatch: got {}, expected {}",
                i,
                v[i],
                expected[i]
            );
        }
    }

    #[test]
    fn test_day_of_week() {
        // 2026-03-11 is Wednesday → Mon=0, Wed=2
        assert_eq!(parse_day_of_week("2026-03-11T18:45:53Z"), 2);
        // 2026-03-14 is Saturday → Mon=0, Sat=5
        assert_eq!(parse_day_of_week("2026-03-14T05:15:12Z"), 5);
    }

    #[test]
    fn test_minutes_diff() {
        let diff = compute_minutes_diff(
            "2026-03-11T20:23:35Z",
            "2026-03-11T14:58:35Z",
        );
        assert!((diff - 325.0).abs() < 0.01);
    }
}
