use crate::models::ReferenceRecord;
use std::fs::File;
use std::path::Path;
use memmap2::Mmap;

/// File header for the binary index file.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IndexHeader {
    pub magic: u32,    // 0x52494E48 = "RINH"
    pub count: u32,    // number of records
    pub dims: u32,     // dimensions per vector (14)
    pub _pad: u32,     // alignment padding
}

unsafe impl bytemuck::Pod for IndexHeader {}
unsafe impl bytemuck::Zeroable for IndexHeader {}

pub const INDEX_MAGIC: u32 = 0x52494E48;

/// A memory-mapped index of reference vectors.
/// The index file layout:
///   [IndexHeader] [ReferenceRecord × count]
pub struct VectorIndex {
    _mmap: Mmap,
    records_ptr: *const ReferenceRecord,
    count: usize,
}

// Safety: The mmap is read-only and the records pointer is derived from it.
// The data is immutable after construction.
unsafe impl Send for VectorIndex {}
unsafe impl Sync for VectorIndex {}

impl VectorIndex {
    /// Load index from a binary file via memory-mapping.
    pub fn load<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };

        let header_size = std::mem::size_of::<IndexHeader>();
        assert!(mmap.len() >= header_size, "Index file too small for header");

        let header: &IndexHeader = bytemuck::from_bytes(&mmap[..header_size]);
        assert_eq!(header.magic, INDEX_MAGIC, "Invalid index magic number");
        assert_eq!(header.dims, 14, "Expected 14 dimensions");

        let count = header.count as usize;
        let record_size = std::mem::size_of::<ReferenceRecord>();
        let expected_size = header_size + count * record_size;
        assert!(
            mmap.len() >= expected_size,
            "Index file truncated: expected {} bytes, got {}",
            expected_size,
            mmap.len()
        );

        let records_ptr = mmap[header_size..].as_ptr() as *const ReferenceRecord;

        Ok(VectorIndex {
            _mmap: mmap,
            records_ptr,
            count,
        })
    }

    /// Get a slice of all reference records (zero-copy from mmap).
    #[inline]
    pub fn records(&self) -> &[ReferenceRecord] {
        unsafe { std::slice::from_raw_parts(self.records_ptr, self.count) }
    }

    /// Number of reference records.
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }
}
