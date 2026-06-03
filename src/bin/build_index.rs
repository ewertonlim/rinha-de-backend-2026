//! Build-time tool: reads references.json.gz and produces a binary VP-Tree index file.
//!
//! Usage: build_index <input.json.gz> <output.bin>
//!
//! The output file has the format:
//!   [IndexHeader (16 bytes)] [ReferenceRecord × N]

use std::env;
use std::fs::File;
use std::io::{BufReader, Write, BufWriter};

use flate2::read::GzDecoder;
use serde::Deserialize;

#[repr(C)]
#[derive(Clone, Copy)]
struct IndexHeader {
    magic: u32,
    count: u32,
    dims: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ReferenceRecord {
    vector: [f32; 14],
    threshold: f32,
    left_child: u32,
    right_child: u32,
    label: u8,
    _pad: [u8; 3],
}

const NULL_CHILD: u32 = u32::MAX;
const _: () = assert!(std::mem::size_of::<ReferenceRecord>() == 72);

#[derive(Deserialize)]
struct JsonRecord {
    vector: Vec<f64>,
    label: String,
}

// Internal structure for building
#[derive(Clone)]
struct Point {
    vector: [f32; 14],
    label: u8,
    dist: f32, // scratch space for distance to current VP
}

#[inline(always)]
fn euclidean_distance(a: &[f32; 14], b: &[f32; 14]) -> f32 {
    let mut sum = 0.0f32;
    let d0 = a[0] - b[0]; let d1 = a[1] - b[1];
    let d2 = a[2] - b[2]; let d3 = a[3] - b[3];
    sum += d0 * d0 + d1 * d1 + d2 * d2 + d3 * d3;

    let d4 = a[4] - b[4]; let d5 = a[5] - b[5];
    let d6 = a[6] - b[6]; let d7 = a[7] - b[7];
    sum += d4 * d4 + d5 * d5 + d6 * d6 + d7 * d7;

    let d8 = a[8] - b[8]; let d9 = a[9] - b[9];
    let d10 = a[10] - b[10]; let d11 = a[11] - b[11];
    sum += d8 * d8 + d9 * d9 + d10 * d10 + d11 * d11;

    let d12 = a[12] - b[12]; let d13 = a[13] - b[13];
    sum += d12 * d12 + d13 * d13;

    sum.sqrt()
}

fn build_vp_tree(points: &mut [Point], nodes: &mut Vec<ReferenceRecord>) -> u32 {
    if points.is_empty() {
        return NULL_CHILD;
    }

    // Use the first point as the Vantage Point (VP)
    let vp = points[0].clone();
    let remaining = &mut points[1..];

    if remaining.is_empty() {
        let idx = nodes.len() as u32;
        nodes.push(ReferenceRecord {
            vector: vp.vector,
            threshold: 0.0,
            left_child: NULL_CHILD,
            right_child: NULL_CHILD,
            label: vp.label,
            _pad: [0; 3],
        });
        return idx;
    }

    // Compute distance from VP to all other points
    for p in remaining.iter_mut() {
        p.dist = euclidean_distance(&vp.vector, &p.vector);
    }

    // Find the median distance
    let median_idx = remaining.len() / 2;
    remaining.select_nth_unstable_by(median_idx, |a, b| a.dist.partial_cmp(&b.dist).unwrap());
    
    let threshold = remaining[median_idx].dist;
    let (inside, outside) = remaining.split_at_mut(median_idx);

    // Reserve index in the array for the current VP node
    let idx = nodes.len() as u32;
    nodes.push(ReferenceRecord {
        vector: vp.vector,
        threshold: 0.0,
        left_child: NULL_CHILD,
        right_child: NULL_CHILD,
        label: vp.label,
        _pad: [0; 3],
    });

    let left = build_vp_tree(inside, nodes);
    let right = build_vp_tree(outside, nodes);

    // Update the node with calculated thresholds and children
    nodes[idx as usize].threshold = threshold;
    nodes[idx as usize].left_child = left;
    nodes[idx as usize].right_child = right;

    idx
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

    let mut points: Vec<Point> = Vec::with_capacity(count);
    for jr in &json_records {
        let mut vector = [0.0f32; 14];
        for i in 0..14 {
            vector[i] = jr.vector[i] as f32;
        }
        let label = if jr.label == "fraud" { 1u8 } else { 0u8 };
        points.push(Point { vector, label, dist: 0.0 });
    }
    
    // Free JSON memory
    drop(json_records);

    eprintln!("Building VP-Tree...");
    let mut records: Vec<ReferenceRecord> = Vec::with_capacity(count);
    let root = build_vp_tree(&mut points, &mut records);
    eprintln!("Tree built! Root node is at index {}", root);
    
    // The root might not be at index 0, so we swap it with 0 so the server can easily find it.
    // By definition, our build algorithm actually puts the very first VP at index 0.
    // Let's assert that.
    assert_eq!(root, 0, "Root must be at index 0");

    eprintln!("Writing {} nodes to {}", records.len(), output_path);
    let out_file = File::create(output_path).expect("Failed to create output file");
    let mut writer = BufWriter::new(out_file);

    let header = IndexHeader {
        magic: 0x52494E48, // "RINH"
        count: count as u32,
        dims: 14,
        _pad: 0,
    };

    // Write header
    let header_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            &header as *const IndexHeader as *const u8,
            std::mem::size_of::<IndexHeader>(),
        )
    };
    writer.write_all(header_bytes).expect("Failed to write header");

    // Write all records
    // We can do it in chunks to avoid single massive allocation, but a single slice works because it's a Vec
    let records_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            records.as_ptr() as *const u8,
            records.len() * std::mem::size_of::<ReferenceRecord>(),
        )
    };
    writer.write_all(records_bytes).expect("Failed to write records");

    writer.flush().expect("Failed to flush output");
    eprintln!("Done! Index written to {}", output_path);
}
