use crate::search::KnnResult;

#[inline]
pub fn decide(knn: &KnnResult) -> (bool, f32) {
    let mut fraud_count = 0.0f32;

    for &(dist, is_fraud) in &knn.neighbors {
        if dist == f32::MAX {
            continue;
        }
        if is_fraud {
            fraud_count += 1.0;
        }
    }

    let fraud_score = fraud_count / 5.0;
    let approved = fraud_score < 0.60;

    (approved, fraud_score)
}
