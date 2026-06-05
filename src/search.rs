use crate::index::VectorIndex;
use std::arch::x86_64::*;

pub const K: usize = 5;
/// Maximum nprobe value supported (stack-allocated array size)
const MAX_NPROBE: usize = 8;

pub struct KnnResult {
    pub neighbors: [(f32, bool); K],
}

#[inline(always)]
fn insert_heap(heap: &mut [(u32, u8); K], heap_max: &mut u32, d_sq: u32, label: u8) {
    if d_sq < *heap_max {
        // Find and replace the current max
        let mut max_idx = 0;
        for i in 1..K {
            if heap[i].0 > heap[max_idx].0 {
                max_idx = i;
            }
        }
        heap[max_idx] = (d_sq, label);
        // Recompute max
        *heap_max = heap.iter().map(|h| h.0).max().unwrap_or(u32::MAX);
    }
}

/// Horizontal sum of 8 x i32 in a __m256i
#[inline(always)]
unsafe fn hsum_epi32(v: __m256i) -> u32 {
    let high = _mm256_extracti128_si256(v, 1);
    let low  = _mm256_castsi256_si128(v);
    let sum128 = _mm_add_epi32(high, low);
    let shuf = _mm_shuffle_epi32(sum128, 0b01_00_11_10);
    let sum64 = _mm_add_epi32(sum128, shuf);
    let shuf2 = _mm_shuffle_epi32(sum64, 0b10_11_00_01);
    let final_ = _mm_add_epi32(sum64, shuf2);
    _mm_cvtsi128_si32(final_) as u32
}

/// Compute squared distance using AVX2 SIMD.
#[inline(always)]
unsafe fn simd_distance_sq(q: __m256i, r_ptr: *const __m256i) -> u32 {
    let r = _mm256_loadu_si256(r_ptr);
    let diff = _mm256_sub_epi16(q, r);
    let sq = _mm256_madd_epi16(diff, diff);
    hsum_epi32(sq)
}

/// IVF-based KNN search — zero heap allocations.
/// 1. Find the `nprobe` closest cluster centroids (stack-allocated mini-heap)
/// 2. Scan all records in those clusters
/// 3. Return K nearest neighbors
pub fn knn_search(
    query: &[i16; 16],
    index: &VectorIndex,
    nprobe: usize,
) -> KnnResult {
    let mut heap: [(u32, u8); K] = [(u32::MAX, 0); K];
    let mut heap_max = u32::MAX;

    if index.len() == 0 {
        return KnnResult { neighbors: [(f32::MAX, false); K] };
    }

    let actual_nprobe = nprobe.min(MAX_NPROBE).min(index.n_clusters());

    // Phase 1: Find closest clusters — stack-allocated, zero allocs
    let mut cluster_heap: [(u32, u16); MAX_NPROBE] = [(u32::MAX, 0); MAX_NPROBE];
    let mut cluster_heap_max = u32::MAX;

    let centroids = index.centroids();
    let n_clusters = centroids.len();

    unsafe {
        let q = _mm256_loadu_si256(query.as_ptr() as *const __m256i);
        let centroids_ptr = centroids.as_ptr() as *const __m256i;

        for c in 0..n_clusters {
            let d_sq = simd_distance_sq(q, centroids_ptr.add(c));
            if d_sq < cluster_heap_max {
                // Insert into cluster mini-heap (replace max)
                let mut max_idx = 0;
                for i in 1..actual_nprobe {
                    if cluster_heap[i].0 > cluster_heap[max_idx].0 {
                        max_idx = i;
                    }
                }
                cluster_heap[max_idx] = (d_sq, c as u16);
                // Recompute max
                cluster_heap_max = 0;
                for i in 0..actual_nprobe {
                    if cluster_heap[i].0 > cluster_heap_max {
                        cluster_heap_max = cluster_heap[i].0;
                    }
                }
            }
        }

        // Phase 2: Scan records in selected clusters
        let all_records_ptr = index.records().as_ptr() as *const __m256i;
        let all_labels_ptr = index.labels().as_ptr();
        let cluster_info = index.cluster_info();

        for probe in 0..actual_nprobe {
            let cluster_id = cluster_heap[probe].1 as usize;
            if cluster_heap[probe].0 == u32::MAX {
                continue; // unused slot
            }

            let entry = &cluster_info[cluster_id];
            let start = entry.offset as usize;
            let count = entry.count as usize;

            let mut i = 0;
            while i < count {
                let idx = start + i;
                // Prefetch 4 vectors ahead to hide L2→L1 latency
                if i + 4 < count {
                    _mm_prefetch::<_MM_HINT_T0>(all_records_ptr.add(idx + 4) as *const i8);
                }
                let d_sq = simd_distance_sq(q, all_records_ptr.add(idx));
                if d_sq < heap_max {
                    insert_heap(&mut heap, &mut heap_max, d_sq, *all_labels_ptr.add(idx));
                }
                i += 1;
            }
        }
    }

    // Convert heap to result
    let mut neighbors = [(f32::MAX, false); K];
    for i in 0..K {
        neighbors[i] = (
            if heap[i].0 == u32::MAX { f32::MAX } else { (heap[i].0 as f32).sqrt() / 10000.0 },
            heap[i].1 == 1,
        );
    }
    KnnResult { neighbors }
}
