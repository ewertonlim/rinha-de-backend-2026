mod decision;
mod index;
mod models;
mod search;
mod vectorize;

use actix_web::{web, App, HttpResponse, HttpServer};
use models::{FraudRequest, FraudResponse, NormalizationConfig};
use std::collections::HashMap;
use std::sync::Arc;

/// Shared application state across all request handlers.
struct AppState {
    index: index::VectorIndex,
    norm: NormalizationConfig,
    mcc_risk: HashMap<String, f32>,
}

/// GET /ready — readiness probe
async fn ready() -> HttpResponse {
    HttpResponse::Ok().finish()
}

/// POST /fraud-score — fraud detection endpoint
async fn fraud_score(
    state: web::Data<Arc<AppState>>,
    body: web::Json<FraudRequest>,
) -> HttpResponse {
    // 1. Vectorize the request payload into 14 dimensions
    let query = vectorize::vectorize(&body, &state.norm, &state.mcc_risk);

    // 2. KNN search for 5 nearest neighbors
    let result = search::knn_search(&query, state.index.records());

    // 3. Decide approved/denied based on fraud_score
    let (approved, fraud_score) = decision::decide(&result);

    // 4. Return response
    HttpResponse::Ok().json(FraudResponse {
        approved,
        fraud_score,
    })
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .expect("PORT must be a number");

    let index_path = std::env::var("INDEX_PATH")
        .unwrap_or_else(|_| "/data/index.bin".to_string());

    let norm_path = std::env::var("NORM_PATH")
        .unwrap_or_else(|_| "/data/normalization.json".to_string());

    let mcc_path = std::env::var("MCC_PATH")
        .unwrap_or_else(|_| "/data/mcc_risk.json".to_string());

    eprintln!("Loading normalization config from {}", norm_path);
    let norm: NormalizationConfig = serde_json::from_str(
        &std::fs::read_to_string(&norm_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", norm_path, e)),
    )
    .expect("Failed to parse normalization.json");

    eprintln!("Loading MCC risk from {}", mcc_path);
    let mcc_risk: HashMap<String, f32> = serde_json::from_str(
        &std::fs::read_to_string(&mcc_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", mcc_path, e)),
    )
    .expect("Failed to parse mcc_risk.json");

    eprintln!("Loading vector index from {}", index_path);
    let index = index::VectorIndex::load(&index_path)
        .unwrap_or_else(|e| panic!("Failed to load index from {}: {}", index_path, e));
    eprintln!("Loaded {} reference vectors", index.len());

    let state = Arc::new(AppState {
        index,
        norm,
        mcc_risk,
    });

    eprintln!("Starting server on port {}", port);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .app_data(
                web::JsonConfig::default()
                    .limit(4096) // Max payload size
            )
            .route("/ready", web::get().to(ready))
            .route("/fraud-score", web::post().to(fraud_score))
    })
    .bind(("0.0.0.0", port))?
    .workers(2)
    .run()
    .await
}
