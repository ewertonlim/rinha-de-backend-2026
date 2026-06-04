use crate::models::{FraudRequest, NormalizationConfig};

pub fn vectorize(
    req: &FraudRequest,
    norm: &NormalizationConfig,
    mcc_risk: &[f32; 10000],
) -> [i16; 16] {
    let mut v = [0f32; 14];
    v[0] = (req.transaction.amount / norm.max_amount) as f32;
    v[1] = (req.transaction.installments as f64 / norm.max_installments) as f32;
    v[2] = ((req.transaction.amount / req.customer.avg_amount) / norm.amount_vs_avg_ratio) as f32;

    let hour = parse_hour(&req.transaction.requested_at);
    v[3] = (hour as f64 / 23.0) as f32;

    let dow = parse_day_of_week(&req.transaction.requested_at);
    v[4] = (dow as f64 / 6.0) as f32;

    match &req.last_transaction {
        Some(last) => {
            let minutes = compute_minutes_diff(
                &req.transaction.requested_at,
                &last.timestamp,
            );
            v[5] = (minutes / norm.max_minutes) as f32;
            v[6] = (last.km_from_current / norm.max_km) as f32;
        }
        None => {
            v[5] = -1.0;
            v[6] = -1.0;
        }
    }

    v[7] = (req.terminal.km_from_home / norm.max_km) as f32;
    v[8] = (req.customer.tx_count_24h as f64 / norm.max_tx_count_24h) as f32;
    v[9] = if req.terminal.is_online { 1.0 } else { 0.0 };
    v[10] = if req.terminal.card_present { 1.0 } else { 0.0 };
    v[11] = if req.customer.known_merchants.contains(&req.merchant.id) { 0.0 } else { 1.0 };

    let mcc_idx = parse_u32_4(req.merchant.mcc.as_bytes());
    let mcc_val = if mcc_idx < 10000 { mcc_risk[mcc_idx as usize] } else { 0.5 };
    v[12] = mcc_val;
    v[13] = (req.merchant.avg_amount / norm.max_merchant_avg_amount) as f32;

    let mut out = [0i16; 16];
    for i in 0..14 {
        if v[i] < 0.0 && i != 5 && i != 6 { v[i] = 0.0; }
        if v[i] > 1.0 { v[i] = 1.0; }
        out[i] = (v[i] * 10000.0).round() as i16;
    }

    out
}

#[inline(always)]
fn parse_u32_2(b: &[u8]) -> u32 {
    if b.len() >= 2 {
        (b[0] - b'0') as u32 * 10 + (b[1] - b'0') as u32
    } else {
        0
    }
}

#[inline(always)]
fn parse_u32_4(b: &[u8]) -> u32 {
    if b.len() >= 4 {
        (b[0] - b'0') as u32 * 1000
            + (b[1] - b'0') as u32 * 100
            + (b[2] - b'0') as u32 * 10
            + (b[3] - b'0') as u32
    } else {
        0
    }
}

#[inline]
fn parse_hour(ts: &str) -> u32 {
    let b = ts.as_bytes();
    if b.len() >= 13 {
        parse_u32_2(&b[11..13])
    } else {
        0
    }
}

#[inline]
fn parse_day_of_week(ts: &str) -> u32 {
    let b = ts.as_bytes();
    if b.len() < 10 { return 0; }
    let y = parse_u32_4(&b[0..4]) as i32;
    let m = parse_u32_2(&b[5..7]);
    let d = parse_u32_2(&b[8..10]);

    let t = [0i32, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut yy = y;
    if m < 3 {
        yy -= 1;
    }
    let m_idx = if m >= 1 && m <= 12 { (m - 1) as usize } else { 0 };
    let dow_sunday_0 =
        ((yy + yy / 4 - yy / 100 + yy / 400 + t[m_idx] + d as i32) % 7) as u32;

    (dow_sunday_0 + 6) % 7
}

#[inline]
fn compute_minutes_diff(current: &str, previous: &str) -> f64 {
    let current_secs = parse_timestamp_to_epoch(current);
    let previous_secs = parse_timestamp_to_epoch(previous);
    let diff_secs = (current_secs - previous_secs).unsigned_abs();
    diff_secs as f64 / 60.0
}

#[inline]
fn parse_timestamp_to_epoch(ts: &str) -> i64 {
    let b = ts.as_bytes();
    if b.len() < 19 { return 0; }
    let y = parse_u32_4(&b[0..4]) as i64;
    let m = parse_u32_2(&b[5..7]);
    let d = parse_u32_2(&b[8..10]);
    let h = parse_u32_2(&b[11..13]);
    let min = parse_u32_2(&b[14..16]);
    let s = parse_u32_2(&b[17..19]);

    let mut days: i64 = 365 * y + y / 4 - y / 100 + y / 400;

    let month_days: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let m_idx = if m >= 1 && m <= 12 { (m - 1) as usize } else { 0 };
    days += month_days[m_idx] as i64;

    if m > 2 && (y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)) {
        days += 1;
    }

    days += d as i64;

    days * 86400 + h as i64 * 3600 + min as i64 * 60 + s as i64
}
