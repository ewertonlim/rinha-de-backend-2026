use crate::models::{ReferenceRecord, ClusterEntry};
use std::fs::File;
use std::path::Path;
use memmap2::Mmap;

/// File header for the IVF binary index file (v2).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IndexHeader {
    pub magic: u32,       // 0x52494E49 = "RINI" (v2 IVF)
    pub count: u32,       // number of records
    pub dims: u32,        // dimensions per vector (14)
    pub n_clusters: u32,  // number of k-means clusters
}

unsafe impl bytemuck::Pod for IndexHeader {}
unsafe impl bytemuck::Zeroable for IndexHeader {}

pub const INDEX_MAGIC: u32 = 0x52494E49; // "RINI" v2

pub struct VectorIndex {
    _mmap: Mmap,
    centroids_ptr: *const ReferenceRecord,
    cluster_info_ptr: *const ClusterEntry,
    records_ptr: *const ReferenceRecord,
    labels_ptr: *const u8,
    count: usize,
    n_clusters: usize,
}

unsafe impl Send for VectorIndex {}
unsafe impl Sync for VectorIndex {}

impl VectorIndex {
    pub fn load<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };

        let header_size = std::mem::size_of::<IndexHeader>();
        assert!(mmap.len() >= header_size, "Index file too small for header");

        let header: &IndexHeader = bytemuck::from_bytes(&mmap[..header_size]);
        assert_eq!(header.magic, INDEX_MAGIC, "Invalid index magic number (expected IVF v2 0x52494E49)");
        assert_eq!(header.dims, 14, "Expected 14 dimensions");

        let count = header.count as usize;
        let n_clusters = header.n_clusters as usize;
        let record_size = std::mem::size_of::<ReferenceRecord>();
        let cluster_entry_size = std::mem::size_of::<ClusterEntry>();

        // Layout: header | centroids | cluster_info | records | labels
        let centroids_size = n_clusters * record_size;
        let cluster_info_size = n_clusters * cluster_entry_size;
        let records_size = count * record_size;
        let labels_size = count;

        let expected_size = header_size + centroids_size + cluster_info_size + records_size + labels_size;
        assert!(
            mmap.len() >= expected_size,
            "Index file truncated: expected {} bytes, got {}",
            expected_size,
            mmap.len()
        );

        let mut offset = header_size;

        let centroids_ptr = mmap[offset..].as_ptr() as *const ReferenceRecord;
        offset += centroids_size;

        let cluster_info_ptr = mmap[offset..].as_ptr() as *const ClusterEntry;
        offset += cluster_info_size;

        let records_ptr = mmap[offset..].as_ptr() as *const ReferenceRecord;
        offset += records_size;

        let labels_ptr = mmap[offset..].as_ptr();

        eprintln!("IVF index loaded: {} records, {} clusters", count, n_clusters);

        Ok(VectorIndex {
            _mmap: mmap,
            centroids_ptr,
            cluster_info_ptr,
            records_ptr,
            labels_ptr,
            count,
            n_clusters,
        })
    }

    #[inline]
    pub fn centroids(&self) -> &[ReferenceRecord] {
        unsafe { std::slice::from_raw_parts(self.centroids_ptr, self.n_clusters) }
    }

    #[inline]
    pub fn cluster_info(&self) -> &[ClusterEntry] {
        unsafe { std::slice::from_raw_parts(self.cluster_info_ptr, self.n_clusters) }
    }

    #[inline]
    pub fn records(&self) -> &[ReferenceRecord] {
        unsafe { std::slice::from_raw_parts(self.records_ptr, self.count) }
    }

    #[inline]
    pub fn labels(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.labels_ptr, self.count) }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    #[inline]
    pub fn n_clusters(&self) -> usize {
        self.n_clusters
    }

    /// Get the records slice for a specific cluster.
    #[inline]
    pub fn cluster_records(&self, cluster_id: usize) -> &[ReferenceRecord] {
        let info = &self.cluster_info()[cluster_id];
        let start = info.offset as usize;
        let count = info.count as usize;
        &self.records()[start..start + count]
    }

    /// Get the labels slice for a specific cluster.
    #[inline]
    pub fn cluster_labels(&self, cluster_id: usize) -> &[u8] {
        let info = &self.cluster_info()[cluster_id];
        let start = info.offset as usize;
        let count = info.count as usize;
        &self.labels()[start..start + count]
    }
}
