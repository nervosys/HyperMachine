//! Copy-on-write guest-memory templates for fast VM cloning.
//!
//! Agent fleets spawn many short-lived VMs from a common warm baseline — a
//! booted OS image, a loaded model runtime, a primed interpreter. Copying
//! gigabytes of RAM per spawn is the cold-start bottleneck.
//!
//! A [`MemoryTemplate`] captures that baseline once. Each [`CowMemory`] clone
//! shares the template's pages read-only and copies an individual page only the
//! first time the guest writes to it. The result:
//!
//! - **O(1) spawn** — [`MemoryTemplate::instantiate`] is constant time in the
//!   baseline size (it clones an [`Arc`] and allocates an empty overlay), so an
//!   agent VM "boots" from a warm image without a multi-gigabyte memcpy.
//! - **Density** — N idle clones cost ~one baseline of RAM regardless of N;
//!   only pages a clone actually dirties become private.
//!
//! This is software copy-on-write: all access goes through [`CowMemory`], so it
//! suits lightweight/agent VMs and snapshot fan-out rather than a vCPU running
//! under hardware-accelerated direct-mapped memory ([`crate::memory::GuestMemory`]).
//! [`CowMemory::materialize`] flattens a clone back into a contiguous image when
//! one is needed (e.g. to hand to a backend).

use crate::memory::GuestMemory;
use crate::{Error, Result};
use std::collections::HashMap;
use std::sync::Arc;

/// Page granularity for copy-on-write sharing (4 KiB), matching the snapshot
/// subsystem's [`crate::snapshot::memory::PAGE_SIZE`].
pub const PAGE_SIZE: usize = 4096;

/// An immutable, shareable baseline image.
///
/// The baseline is padded up to a whole number of [`PAGE_SIZE`] pages so every
/// clone page is exactly `PAGE_SIZE` bytes; `len` records the logical length so
/// reads and writes are bounds-checked against the original size.
pub struct MemoryTemplate {
    /// Page-aligned baseline bytes (length is a multiple of `PAGE_SIZE`).
    data: Vec<u8>,
    /// Logical length (<= `data.len()`).
    len: usize,
}

impl MemoryTemplate {
    /// Build a template from a byte image.
    pub fn from_bytes(bytes: &[u8]) -> Arc<Self> {
        let len = bytes.len();
        let padded = len.div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let mut data = Vec::with_capacity(padded);
        data.extend_from_slice(bytes);
        data.resize(padded, 0);
        Arc::new(Self { data, len })
    }

    /// Capture a live [`GuestMemory`] into a template by reading the bytes of
    /// each allocated region into their guest-physical offsets. Gaps between
    /// regions read back as zero.
    pub fn capture(mem: &GuestMemory) -> Result<Arc<Self>> {
        let regions = mem.regions();
        // Size the baseline to cover both the configured total and any region
        // that extends past it.
        let span = regions
            .iter()
            .map(|r| r.guest_addr + r.size)
            .max()
            .unwrap_or(0)
            .max(mem.total_size());
        let len = span as usize;
        let padded = len.div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let mut data = vec![0u8; padded];
        for r in &regions {
            let start = r.guest_addr as usize;
            let end = start + r.size as usize;
            mem.read_bytes_into(r.guest_addr, &mut data[start..end])?;
        }
        Ok(Arc::new(Self { data, len }))
    }

    /// Logical length of the baseline in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the baseline is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of pages in the baseline.
    pub fn page_count(&self) -> usize {
        self.data.len() / PAGE_SIZE
    }

    /// Bytes of one baseline page (always `PAGE_SIZE` long).
    fn page(&self, idx: usize) -> &[u8] {
        let start = idx * PAGE_SIZE;
        &self.data[start..start + PAGE_SIZE]
    }

    /// Instantiate a copy-on-write clone. Constant time in the baseline size.
    pub fn instantiate(self: &Arc<Self>) -> CowMemory {
        CowMemory {
            template: Arc::clone(self),
            overlay: HashMap::new(),
        }
    }

    /// How many live clones currently share this template.
    pub fn clone_count(self: &Arc<Self>) -> usize {
        // Subtract the caller's own reference.
        Arc::strong_count(self).saturating_sub(1)
    }
}

/// A copy-on-write view over a shared [`MemoryTemplate`].
///
/// Reads fall through to the template; the first write to a page copies that
/// page private. Only dirtied pages occupy clone-private RAM.
pub struct CowMemory {
    template: Arc<MemoryTemplate>,
    /// Pages that have diverged from the template, keyed by page index.
    overlay: HashMap<usize, Vec<u8>>,
}

impl CowMemory {
    /// Logical length in bytes (same as the template).
    pub fn len(&self) -> usize {
        self.template.len
    }

    /// Whether the memory is empty.
    pub fn is_empty(&self) -> bool {
        self.template.len == 0
    }

    /// Bytes resident (private) to this clone — the copied pages.
    pub fn resident_bytes(&self) -> usize {
        self.overlay.len() * PAGE_SIZE
    }

    /// Bytes still shared with the template (not yet copied).
    pub fn shared_bytes(&self) -> usize {
        (self.template.page_count() - self.overlay.len()) * PAGE_SIZE
    }

    /// Number of pages copied-on-write so far.
    pub fn copied_pages(&self) -> usize {
        self.overlay.len()
    }

    fn check_bounds(&self, offset: usize, len: usize) -> Result<()> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| Error::Memory(format!("CoW access overflow: {offset} + {len}")))?;
        if end > self.template.len {
            return Err(Error::Memory(format!(
                "CoW access {offset}..{end} exceeds memory length {}",
                self.template.len
            )));
        }
        Ok(())
    }

    /// Read the current bytes of one page (overlay if dirtied, else template).
    fn page_bytes(&self, idx: usize) -> &[u8] {
        match self.overlay.get(&idx) {
            Some(p) => p,
            None => self.template.page(idx),
        }
    }

    /// Read `buf.len()` bytes starting at `offset` into `buf`.
    pub fn read_into(&self, offset: usize, buf: &mut [u8]) -> Result<()> {
        self.check_bounds(offset, buf.len())?;
        if self.overlay.is_empty() {
            // Fast path: no page has diverged from the template yet (the common
            // case for an idle clone) — one contiguous copy, no per-page lookups.
            buf.copy_from_slice(&self.template.data[offset..offset + buf.len()]);
            return Ok(());
        }
        let mut pos = offset;
        let mut done = 0;
        while done < buf.len() {
            let page = pos / PAGE_SIZE;
            let po = pos % PAGE_SIZE;
            let n = (PAGE_SIZE - po).min(buf.len() - done);
            buf[done..done + n].copy_from_slice(&self.page_bytes(page)[po..po + n]);
            pos += n;
            done += n;
        }
        Ok(())
    }

    /// Read `len` bytes starting at `offset`.
    pub fn read(&self, offset: usize, len: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        self.read_into(offset, &mut buf)?;
        Ok(buf)
    }

    /// Write `data` at `offset`, copying any touched template page private first.
    pub fn write(&mut self, offset: usize, data: &[u8]) -> Result<()> {
        self.check_bounds(offset, data.len())?;
        let mut pos = offset;
        let mut done = 0;
        while done < data.len() {
            let page = pos / PAGE_SIZE;
            let po = pos % PAGE_SIZE;
            let n = (PAGE_SIZE - po).min(data.len() - done);
            // Copy-on-write: materialize the page privately on first write (a
            // single hash lookup via `entry`).
            let template = &self.template;
            let pg = self
                .overlay
                .entry(page)
                .or_insert_with(|| template.page(page).to_vec());
            pg[po..po + n].copy_from_slice(&data[done..done + n]);
            pos += n;
            done += n;
        }
        Ok(())
    }

    /// Flatten the current state into a contiguous image of [`Self::len`] bytes
    /// (overlay pages override template pages). Use this when a clone must be
    /// handed to a backend as a plain buffer.
    pub fn materialize(&self) -> Vec<u8> {
        let mut out = self.template.data.clone();
        for (&idx, page) in &self.overlay {
            let start = idx * PAGE_SIZE;
            out[start..start + PAGE_SIZE].copy_from_slice(page);
        }
        out.truncate(self.template.len);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn clone_shares_baseline_reads() {
        let base = pattern(3 * PAGE_SIZE + 17);
        let tmpl = MemoryTemplate::from_bytes(&base);
        let clone = tmpl.instantiate();

        assert_eq!(clone.len(), base.len());
        assert_eq!(clone.read(0, base.len()).unwrap(), base);
        // Nothing written yet: everything is shared, nothing resident.
        assert_eq!(clone.copied_pages(), 0);
        assert_eq!(clone.resident_bytes(), 0);
    }

    #[test]
    fn write_is_copy_on_write_and_isolated() {
        let base = pattern(4 * PAGE_SIZE);
        let tmpl = MemoryTemplate::from_bytes(&base);
        let mut a = tmpl.instantiate();
        let b = tmpl.instantiate();

        // Write into page 1 of clone a.
        a.write(PAGE_SIZE, &[0xAB; 8]).unwrap();

        // a sees its write; exactly one page became private.
        assert_eq!(a.read(PAGE_SIZE, 8).unwrap(), vec![0xAB; 8]);
        assert_eq!(a.copied_pages(), 1);
        assert_eq!(a.resident_bytes(), PAGE_SIZE);

        // Untouched pages of a still read the baseline.
        assert_eq!(a.read(0, 8).unwrap(), base[0..8]);

        // Sibling b and the template are unaffected (true isolation).
        assert_eq!(
            b.read(PAGE_SIZE, 8).unwrap(),
            base[PAGE_SIZE..PAGE_SIZE + 8]
        );
        assert_eq!(b.copied_pages(), 0);
    }

    #[test]
    fn write_spanning_page_boundary_copies_two_pages() {
        let base = pattern(4 * PAGE_SIZE);
        let tmpl = MemoryTemplate::from_bytes(&base);
        let mut c = tmpl.instantiate();

        // Straddle the boundary between page 0 and page 1.
        let off = PAGE_SIZE - 4;
        c.write(off, &[0xFF; 8]).unwrap();
        assert_eq!(c.copied_pages(), 2);
        assert_eq!(c.read(off, 8).unwrap(), vec![0xFF; 8]);
        // Page 2 still shared.
        assert_eq!(
            c.read(2 * PAGE_SIZE, 4).unwrap(),
            base[2 * PAGE_SIZE..2 * PAGE_SIZE + 4]
        );
    }

    #[test]
    fn materialize_reflects_writes() {
        let base = pattern(2 * PAGE_SIZE + 100);
        let tmpl = MemoryTemplate::from_bytes(&base);
        let mut c = tmpl.instantiate();
        c.write(10, &[0x01, 0x02, 0x03]).unwrap();

        let mut expected = base.clone();
        expected[10..13].copy_from_slice(&[0x01, 0x02, 0x03]);
        assert_eq!(c.materialize(), expected);
    }

    #[test]
    fn out_of_bounds_access_errors() {
        let tmpl = MemoryTemplate::from_bytes(&pattern(100));
        let mut c = tmpl.instantiate();
        assert!(c.read(90, 20).is_err());
        assert!(c.write(95, &[0; 10]).is_err());
        // In-bounds edge is fine.
        c.write(99, &[0x7F]).unwrap();
        assert_eq!(c.read(99, 1).unwrap(), vec![0x7F]);
    }

    #[test]
    fn many_clones_share_one_baseline() {
        let tmpl = MemoryTemplate::from_bytes(&pattern(64 * PAGE_SIZE));
        let clones: Vec<_> = (0..100).map(|_| tmpl.instantiate()).collect();
        assert_eq!(tmpl.clone_count(), 100);
        // Idle clones hold no private RAM — all pages shared.
        let resident: usize = clones.iter().map(|c| c.resident_bytes()).sum();
        assert_eq!(resident, 0);
    }

    #[test]
    fn capture_from_guest_memory_round_trips() {
        let mem = GuestMemory::new(2 * PAGE_SIZE as u64).unwrap();
        let addr = mem.allocate_region(PAGE_SIZE as u64, false).unwrap();
        mem.write_bytes(addr, &[0x42; 16]).unwrap();

        let tmpl = MemoryTemplate::capture(&mem).unwrap();
        let clone = tmpl.instantiate();
        assert_eq!(clone.read(addr as usize, 16).unwrap(), vec![0x42; 16]);
    }
}
