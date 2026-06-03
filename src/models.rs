use serde::{Deserialize, Serialize};

/// Incoming fraud-check request payload.
#[derive(Deserialize)]
pub struct FraudRequest {
    #[allow(dead_code)]
    pub id: String,
    pub transaction: Transaction,
    pub customer: Customer,
    pub merchant: Merchant,
    pub terminal: Terminal,
    pub last_transaction: Option<LastTransaction>,
}

#[derive(Deserialize)]
pub struct Transaction {
    pub amount: f64,
    pub installments: u32,
    pub requested_at: String,
}

#[derive(Deserialize)]
pub struct Customer {
    pub avg_amount: f64,
    pub tx_count_24h: u32,
    pub known_merchants: Vec<String>,
}

#[derive(Deserialize)]
pub struct Merchant {
    pub id: String,
    pub mcc: String,
    pub avg_amount: f64,
}

#[derive(Deserialize)]
pub struct Terminal {
    pub is_online: bool,
    pub card_present: bool,
    pub km_from_home: f64,
}

#[derive(Deserialize)]
pub struct LastTransaction {
    pub timestamp: String,
    pub km_from_current: f64,
}

/// Response payload.
#[derive(Serialize)]
pub struct FraudResponse {
    pub approved: bool,
    pub fraud_score: f32,
}

/// Normalization constants loaded from normalization.json.
#[derive(Deserialize, Clone)]
pub struct NormalizationConfig {
    pub max_amount: f64,
    pub max_installments: f64,
    pub amount_vs_avg_ratio: f64,
    pub max_minutes: f64,
    pub max_km: f64,
    pub max_tx_count_24h: f64,
    pub max_merchant_avg_amount: f64,
}

/// A single reference record / VP-Tree node.
/// Layout: 14 × f32 (56 bytes) + 1 f32 (4 bytes) + 2 u32 (8 bytes) + u8 label + 3 bytes padding = 72 bytes total.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ReferenceRecord {
    pub vector: [f32; 14],
    pub threshold: f32,
    pub left_child: u32,
    pub right_child: u32,
    pub label: u8, // 1 = fraud, 0 = legit
    pub _pad: [u8; 3],
}

pub const NULL_CHILD: u32 = u32::MAX;

const _: () = assert!(std::mem::size_of::<ReferenceRecord>() == 72);

// Safety: ReferenceRecord is repr(C) with only primitive fields and explicit padding.
unsafe impl bytemuck::Pod for ReferenceRecord {}
unsafe impl bytemuck::Zeroable for ReferenceRecord {}
