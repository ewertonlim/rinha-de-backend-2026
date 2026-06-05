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
    mcc_risk: Box<[f32; 10000]>,
    nprobe: usize,
}



/// GET /ready — readiness probe
async fn ready() -> HttpResponse {
    HttpResponse::Ok().finish()
}

async fn fraud_score(
    state: web::Data<Arc<AppState>>,
    body: web::Bytes,
) -> HttpResponse {
    let req: FraudRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(_) => return HttpResponse::BadRequest().finish(),
    };

    // 1. Vectorize the request payload into 14 dimensions
    let query = vectorize::vectorize(&req, &state.norm, &state.mcc_risk);

    // 2. IVF KNN search
    let result = search::knn_search(&query, &state.index, state.nprobe);

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

    let nprobe: usize = std::env::var("NPROBE")
        .unwrap_or_else(|_| "3".to_string())
        .parse()
        .expect("NPROBE must be a number");

    eprintln!("Loading normalization config from {}", norm_path);
    let norm: NormalizationConfig = serde_json::from_str(
        &std::fs::read_to_string(&norm_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", norm_path, e)),
    )
    .expect("Failed to parse normalization.json");

    eprintln!("Loading MCC risk from {}", mcc_path);
    let mcc_risk_map: HashMap<String, f32> = serde_json::from_str(
        &std::fs::read_to_string(&mcc_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", mcc_path, e)),
    )
    .expect("Failed to parse mcc_risk.json");

    let mut mcc_risk = Box::new([0.5f32; 10000]);
    for (k, v) in mcc_risk_map {
        if let Ok(idx) = k.parse::<usize>() {
            if idx < 10000 {
                mcc_risk[idx] = v;
            }
        }
    }

    eprintln!("Loading vector index from {}", index_path);
    let index = index::VectorIndex::load(&index_path)
        .unwrap_or_else(|e| panic!("Failed to load index from {}: {}", index_path, e));
    eprintln!("Loaded {} reference vectors, {} clusters", index.len(), index.n_clusters());

    eprintln!("NPROBE={}", nprobe);

    let state = Arc::new(AppState {
        index,
        norm,
        mcc_risk,
        nprobe,
    });

    let api_socket = std::env::var("API_SOCKET").ok();

    if let Some(ref path) = api_socket {
        eprintln!("Starting server on unix socket {}", path);
        let _ = std::fs::remove_file(path);
        
        // Ensure parent directory is accessible by nginx user
        #[cfg(unix)]
        if let Some(parent) = std::path::Path::new(path).parent() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o777));
        }
    } else {
        eprintln!("Starting server on port {}", port);
    }

    let mut server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .app_data(
                web::JsonConfig::default()
                    .limit(4096) // Max payload size
            )
            .route("/ready", web::get().to(ready))
            .route("/fraud-score", web::post().to(fraud_score))
    });

    #[cfg(unix)]
    {
        if let Some(path) = api_socket {
            server = server.bind_uds(&path)?.bind(("127.0.0.1", port))?;
            // allow nginx to connect
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o777));
        } else {
            server = server.bind(("0.0.0.0", port))?;
        }
    }

    #[cfg(not(unix))]
    {
        server = server.bind(("0.0.0.0", port))?;
    }

    server.workers(1).run()
    .await
}
