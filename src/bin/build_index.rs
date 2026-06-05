use std::env;
use std::fs::File;
use std::io::{BufReader, Write, BufWriter};

use flate2::read::GzDecoder;
use serde::Deserialize;

// --- Binary format structs ---

#[repr(C)]
#[derive(Clone, Copy)]
struct IndexHeader {
    magic: u32,       // 0x52494E49 = "RINI" (v2 IVF)
    count: u32,       // number of records
    dims: u32,        // dimensions per vector (14)
    n_clusters: u32,  // number of k-means clusters
}

#[repr(C, align(32))]
#[derive(Clone, Copy)]
struct ReferenceRecord {
    vector: [i16; 16], // 32 bytes (14 used + 2 padding)
}

const _: () = assert!(std::mem::size_of::<ReferenceRecord>() == 32);

#[repr(C)]
#[derive(Clone, Copy)]
struct ClusterEntry {
    offset: u32,
    count: u32,
}

// --- JSON input ---

#[derive(Deserialize)]
struct JsonRecord {
    vector: Vec<f64>,
    label: String,
}

// --- K-means ---

const N_CLUSTERS: usize = 1024;
const K_MEANS_ITERS: usize = 12;

/// Compute squared L2 distance between two i16 vectors (first 14 dims).
/// Uses i32 arithmetic to avoid overflow.
#[inline]
fn distance_sq(a: &[i16; 16], b: &[i16; 16]) -> u64 {
    let mut sum: u64 = 0;
    for i in 0..14 {
        let d = a[i] as i32 - b[i] as i32;
        sum += (d * d) as u64;
    }
    sum
}

/// Simple deterministic pseudo-random number generator (xorshift64).
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    /// Random index in [0, n)
    fn next_usize(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn run_kmeans(
    records: &[ReferenceRecord],
    n_clusters: usize,
    iterations: usize,
) -> (Vec<ReferenceRecord>, Vec<u32>) {
    let n = records.len();
    eprintln!("Running k-means: {} clusters, {} iterations, {} records", n_clusters, iterations, n);

    // Initialize centroids by sampling records with fixed seed
    let mut rng = Rng::new(42);
    let mut centroids: Vec<ReferenceRecord> = Vec::with_capacity(n_clusters);
    let mut chosen = std::collections::HashSet::new();
    while centroids.len() < n_clusters {
        let idx = rng.next_usize(n);
        if chosen.insert(idx) {
            centroids.push(records[idx]);
        }
    }

    // Assignments: cluster id for each record
    let mut assignments: Vec<u32> = vec![0u32; n];

    for iter in 0..iterations {
        let t0 = std::time::Instant::now();

        // Assignment step: assign each record to nearest centroid
        for i in 0..n {
            let mut best_dist = u64::MAX;
            let mut best_c = 0u32;
            for c in 0..n_clusters {
                let d = distance_sq(&records[i].vector, &centroids[c].vector);
                if d < best_dist {
                    best_dist = d;
                    best_c = c as u32;
                }
            }
            assignments[i] = best_c;
        }

        // Update step: recompute centroids as mean of assigned records
        // Use i64 accumulators to avoid overflow (3M × 10000 fits in i64)
        let mut sums = vec![[0i64; 14]; n_clusters];
        let mut counts = vec![0u64; n_clusters];

        for i in 0..n {
            let c = assignments[i] as usize;
            counts[c] += 1;
            for d in 0..14 {
                sums[c][d] += records[i].vector[d] as i64;
            }
        }

        for c in 0..n_clusters {
            if counts[c] == 0 {
                // Empty cluster: re-seed with a random record
                let idx = rng.next_usize(n);
                centroids[c] = records[idx];
            } else {
                for d in 0..14 {
                    centroids[c].vector[d] = (sums[c][d] / counts[c] as i64) as i16;
                }
                // Zero padding dims
                centroids[c].vector[14] = 0;
                centroids[c].vector[15] = 0;
            }
        }

        let elapsed = t0.elapsed();
        eprintln!("  k-means iter {}/{}: {:.1}s", iter + 1, iterations, elapsed.as_secs_f64());
    }

    (centroids, assignments)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: build_index <input.json.gz> <output.bin>");
        std::process::exit(1);
    }

    let input_path = &args[1];
    let output_path = &args[2];

    eprintln!("Reading {}", input_path);
    let file = File::open(input_path).expect("Failed to open input file");
    let decoder = GzDecoder::new(BufReader::new(file));
    let reader = BufReader::new(decoder);

    eprintln!("Parsing JSON...");
    let json_records: Vec<JsonRecord> =
        serde_json::from_reader(reader).expect("Failed to parse JSON");

    let count = json_records.len();
    eprintln!("Parsed {} records", count);

    // Convert to ReferenceRecord + label
    let mut records: Vec<ReferenceRecord> = Vec::with_capacity(count);
    let mut labels: Vec<u8> = Vec::with_capacity(count);

    for jr in &json_records {
        let mut vector = [0i16; 16];
        for i in 0..14 {
            let mut val = jr.vector[i] as f32;
            if val < 0.0 && i != 5 && i != 6 { val = 0.0; }
            if val > 1.0 { val = 1.0; }
            vector[i] = (val * 10000.0).round() as i16;
        }
        records.push(ReferenceRecord { vector });
        labels.push(if jr.label == "fraud" { 1u8 } else { 0u8 });
    }
    drop(json_records); // Free JSON memory

    // Run k-means clustering
    let (centroids, assignments) = run_kmeans(&records, N_CLUSTERS, K_MEANS_ITERS);

    // Sort records by cluster assignment
    eprintln!("Sorting records by cluster...");
    let mut indices: Vec<usize> = (0..count).collect();
    indices.sort_unstable_by_key(|&i| assignments[i]);

    let sorted_records: Vec<ReferenceRecord> = indices.iter().map(|&i| records[i]).collect();
    let sorted_labels: Vec<u8> = indices.iter().map(|&i| labels[i]).collect();

    // Compute cluster boundaries
    let mut cluster_entries = vec![ClusterEntry { offset: 0, count: 0 }; N_CLUSTERS];
    {
        let mut current_cluster = assignments[indices[0]] as usize;
        let mut current_start = 0usize;
        let mut current_count = 0u32;

        for (pos, &idx) in indices.iter().enumerate() {
            let c = assignments[idx] as usize;
            if c != current_cluster {
                cluster_entries[current_cluster] = ClusterEntry {
                    offset: current_start as u32,
                    count: current_count,
                };
                current_cluster = c;
                current_start = pos;
                current_count = 0;
            }
            current_count += 1;
        }
        // Last cluster
        cluster_entries[current_cluster] = ClusterEntry {
            offset: current_start as u32,
            count: current_count,
        };
    }

    // Log cluster size stats
    let sizes: Vec<u32> = cluster_entries.iter().map(|e| e.count).collect();
    let min_size = sizes.iter().copied().min().unwrap_or(0);
    let max_size = sizes.iter().copied().max().unwrap_or(0);
    let avg_size = count as f64 / N_CLUSTERS as f64;
    let empty = sizes.iter().filter(|&&s| s == 0).count();
    eprintln!("Cluster sizes: min={}, max={}, avg={:.0}, empty={}", min_size, max_size, avg_size, empty);

    // Write binary index
    eprintln!("Writing IVF index to {}", output_path);
    let out_file = File::create(output_path).expect("Failed to create output file");
    let mut writer = BufWriter::new(out_file);

    // Header
    let header = IndexHeader {
        magic: 0x52494E49, // "RINI" v2
        count: count as u32,
        dims: 14,
        n_clusters: N_CLUSTERS as u32,
    };
    let header_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            &header as *const IndexHeader as *const u8,
            std::mem::size_of::<IndexHeader>(),
        )
    };
    writer.write_all(header_bytes).expect("Failed to write header");

    // Centroids
    let centroids_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            centroids.as_ptr() as *const u8,
            centroids.len() * std::mem::size_of::<ReferenceRecord>(),
        )
    };
    writer.write_all(centroids_bytes).expect("Failed to write centroids");

    // Cluster entries
    let cluster_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            cluster_entries.as_ptr() as *const u8,
            cluster_entries.len() * std::mem::size_of::<ClusterEntry>(),
        )
    };
    writer.write_all(cluster_bytes).expect("Failed to write cluster entries");

    // Records (sorted by cluster)
    let records_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            sorted_records.as_ptr() as *const u8,
            sorted_records.len() * std::mem::size_of::<ReferenceRecord>(),
        )
    };
    writer.write_all(records_bytes).expect("Failed to write records");

    // Labels (same order)
    writer.write_all(&sorted_labels).expect("Failed to write labels");

    writer.flush().expect("Failed to flush output");
    eprintln!("Done! IVF index written to {} ({} clusters, {} records)", output_path, N_CLUSTERS, count);
}
