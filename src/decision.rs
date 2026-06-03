use crate::search::KnnResult;

/// Decides whether a transaction should be approved based on KNN results.
///
/// fraud_score = number_of_frauds_in_top5 / 5
/// approved = fraud_score < 0.6 (i.e., fewer than 3 out of 5 are fraud)
#[inline]
pub fn decide(result: &KnnResult) -> (bool, f32) {
    let fraud_count = result
        .neighbors
        .iter()
        .filter(|(_, is_fraud)| *is_fraud)
        .count();
    let fraud_score = fraud_count as f32 / 5.0;
    let approved = fraud_score < 0.6;
    (approved, fraud_score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_legit() {
        let result = KnnResult {
            neighbors: [
                (0.1, false),
                (0.2, false),
                (0.3, false),
                (0.4, false),
                (0.5, false),
            ],
        };
        let (approved, score) = decide(&result);
        assert!(approved);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_all_fraud() {
        let result = KnnResult {
            neighbors: [
                (0.1, true),
                (0.2, true),
                (0.3, true),
                (0.4, true),
                (0.5, true),
            ],
        };
        let (approved, score) = decide(&result);
        assert!(!approved);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn test_threshold_boundary() {
        // 3 out of 5 = 0.6 → NOT approved (threshold is strict <)
        let result = KnnResult {
            neighbors: [
                (0.1, true),
                (0.2, true),
                (0.3, true),
                (0.4, false),
                (0.5, false),
            ],
        };
        let (approved, score) = decide(&result);
        assert!(!approved);
        assert_eq!(score, 0.6);

        // 2 out of 5 = 0.4 → approved
        let result2 = KnnResult {
            neighbors: [
                (0.1, true),
                (0.2, true),
                (0.3, false),
                (0.4, false),
                (0.5, false),
            ],
        };
        let (approved2, score2) = decide(&result2);
        assert!(approved2);
        assert_eq!(score2, 0.4);
    }
}
