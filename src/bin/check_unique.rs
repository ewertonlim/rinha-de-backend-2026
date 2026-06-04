use std::fs::File;
use std::io::Read;
use std::collections::HashMap;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ReferenceRecord {
    pub vector: [u8; 14],
    pub label: u8,
    pub _pad: u8,
    pub left_child: u32,
    pub right_child: u32,
    pub threshold: f32,
}

fn main() {
    let mut file = File::open("/data/index.bin").unwrap();
    let mut header = [0u8; 16];
    file.read_exact(&mut header).unwrap();
    
    let mut data = Vec::new();
    file.read_to_end(&mut data).unwrap();
    
    let ptr = data.as_ptr() as *const ReferenceRecord;
    let count = data.len() / std::mem::size_of::<ReferenceRecord>();
    let records = unsafe { std::slice::from_raw_parts(ptr, count) };
    
    let mut map = HashMap::new();
    for r in records {
        let entry = map.entry(r.vector).or_insert((0u32, 0u32));
        if r.label == 1 {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }
    
    println!("Total records: {}", count);
    println!("Unique vectors: {}", map.len());
}
