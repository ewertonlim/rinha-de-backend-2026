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

#[repr(C, align(32))]
#[derive(Debug, Clone, Copy)]
pub struct ReferenceRecord {
    pub vector: [i16; 16], // 32 bytes
}

const _: () = assert!(std::mem::size_of::<ReferenceRecord>() == 32);

// Safety: ReferenceRecord is repr(C) with only primitive fields and explicit padding.
unsafe impl bytemuck::Pod for ReferenceRecord {}
unsafe impl bytemuck::Zeroable for ReferenceRecord {}

/// Cluster metadata for IVF index: offset and count of records in each cluster.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ClusterEntry {
    pub offset: u32,
    pub count: u32,
}

const _: () = assert!(std::mem::size_of::<ClusterEntry>() == 8);

unsafe impl bytemuck::Pod for ClusterEntry {}
unsafe impl bytemuck::Zeroable for ClusterEntry {}
