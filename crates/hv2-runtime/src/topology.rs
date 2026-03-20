//! GPU Topology-Aware Scheduling
//!
//! Extends the base scheduler with GPU interconnect, NUMA, and multi-GPU
//! awareness. Understands NVLink meshes, PCIe switch hierarchies, and
//! NUMA distances so that training workloads land on GPUs with the best
//! peer-to-peer bandwidth and lowest memory-access latency.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Interconnect type between two GPUs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GpuInterconnect {
    /// NVLink (high bandwidth, low latency)
    NvLink,
    /// NVSwitch fabric (all-to-all NVLink)
    NvSwitch,
    /// PCIe peer-to-peer (same switch)
    PciePeer,
    /// PCIe via CPU root complex
    PcieRoot,
    /// Cross-NUMA (remote memory bus)
    CrossNuma,
}

impl GpuInterconnect {
    /// Approximate unidirectional bandwidth in GB/s
    pub fn bandwidth_gbps(&self) -> f64 {
        match self {
            Self::NvLink => 300.0,   // NVLink 4.0 per sub-link
            Self::NvSwitch => 900.0, // NVSwitch 3 aggregate
            Self::PciePeer => 32.0,  // PCIe 5.0 x16
            Self::PcieRoot => 16.0,  // Traverses root complex
            Self::CrossNuma => 8.0,  // Cross-socket with added latency
        }
    }

    /// Score (0.0–1.0) used by the topology-aware scorer
    pub fn affinity_score(&self) -> f64 {
        match self {
            Self::NvSwitch => 1.0,
            Self::NvLink => 0.9,
            Self::PciePeer => 0.6,
            Self::PcieRoot => 0.3,
            Self::CrossNuma => 0.1,
        }
    }
}

/// Physical GPU descriptor in the fleet inventory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuDevice {
    /// Unique device ID (e.g., "gpu-0000:3b:00.0")
    pub id: String,
    /// Host / node the GPU is installed in
    pub host_id: String,
    /// NUMA node index
    pub numa_node: u32,
    /// PCIe BDF address
    pub pci_address: String,
    /// GPU model name
    pub model: String,
    /// VRAM in bytes
    pub vram_bytes: u64,
    /// Compute capability (e.g., 90 for sm_90)
    pub compute_capability: u32,
    /// Current utilization (0.0–1.0)
    pub utilization: f64,
    /// Whether this device is currently allocated
    pub allocated: bool,
}

impl GpuDevice {
    /// Create a new GPU device descriptor
    pub fn new(
        id: impl Into<String>,
        host_id: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            host_id: host_id.into(),
            numa_node: 0,
            pci_address: String::new(),
            model: model.into(),
            vram_bytes: 80 * 1024 * 1024 * 1024, // 80 GB default
            compute_capability: 90,
            utilization: 0.0,
            allocated: false,
        }
    }

    /// Set NUMA node
    pub fn numa(mut self, node: u32) -> Self {
        self.numa_node = node;
        self
    }

    /// Set PCIe address
    pub fn pci(mut self, addr: impl Into<String>) -> Self {
        self.pci_address = addr.into();
        self
    }

    /// Set VRAM
    pub fn vram(mut self, bytes: u64) -> Self {
        self.vram_bytes = bytes;
        self
    }

    /// Set compute capability
    pub fn capability(mut self, cc: u32) -> Self {
        self.compute_capability = cc;
        self
    }
}

/// GPU topology link between two devices
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyLink {
    /// Source GPU
    pub from: String,
    /// Destination GPU
    pub to: String,
    /// Interconnect type
    pub interconnect: GpuInterconnect,
    /// Number of links (e.g., 12 NVLinks between two A100s on DGX)
    pub link_count: u32,
}

/// GPU requirements for a workload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuRequirements {
    /// Number of GPUs needed
    pub gpu_count: u32,
    /// Minimum VRAM per GPU in bytes
    pub min_vram_bytes: u64,
    /// Minimum compute capability
    pub min_compute_capability: u32,
    /// Require all GPUs on the same NUMA node
    pub same_numa: bool,
    /// Require NVLink interconnect between all GPUs
    pub require_nvlink: bool,
    /// Prefer co-located GPUs (same host)
    pub prefer_colocated: bool,
    /// Minimum aggregate interconnect bandwidth in GB/s
    pub min_bandwidth_gbps: f64,
}

impl Default for GpuRequirements {
    fn default() -> Self {
        Self {
            gpu_count: 1,
            min_vram_bytes: 0,
            min_compute_capability: 0,
            same_numa: false,
            require_nvlink: false,
            prefer_colocated: true,
            min_bandwidth_gbps: 0.0,
        }
    }
}

impl GpuRequirements {
    /// Builder: set GPU count
    pub fn count(mut self, n: u32) -> Self {
        self.gpu_count = n;
        self
    }

    /// Builder: set minimum VRAM per GPU
    pub fn min_vram(mut self, bytes: u64) -> Self {
        self.min_vram_bytes = bytes;
        self
    }

    /// Builder: require NVLink
    pub fn nvlink(mut self) -> Self {
        self.require_nvlink = true;
        self
    }

    /// Builder: require same NUMA node
    pub fn same_numa_node(mut self) -> Self {
        self.same_numa = true;
        self
    }
}

/// Result of topology-aware GPU placement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuPlacement {
    /// Selected GPU device IDs
    pub gpu_ids: Vec<String>,
    /// Host they reside on
    pub host_id: String,
    /// Overall affinity score (0.0–1.0)
    pub affinity_score: f64,
    /// Aggregate interconnect bandwidth among selected GPUs (GB/s)
    pub aggregate_bandwidth_gbps: f64,
    /// Whether all GPUs are on the same NUMA node
    pub same_numa: bool,
}

/// Topology-aware placement errors
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TopologyError {
    /// Not enough GPUs to satisfy the request
    #[error("Insufficient GPUs: need {needed}, available {available}")]
    InsufficientGpus { needed: u32, available: u32 },

    /// No group of GPUs satisfies interconnect requirements
    #[error("No GPU group satisfies interconnect requirements: {0}")]
    InterconnectUnsatisfied(String),

    /// Requested compute capability not available
    #[error("Compute capability {requested} not available (max: {available})")]
    ComputeCapabilityUnavailable { requested: u32, available: u32 },

    /// No GPUs with enough VRAM
    #[error("Insufficient VRAM: need {needed} bytes, max available {available} bytes")]
    InsufficientVram { needed: u64, available: u64 },
}

/// Result alias
pub type TopologyResult<T> = Result<T, TopologyError>;

/// Fleet GPU topology map
///
/// Holds the global view of all GPUs and their interconnects.
/// The scheduler queries this to make topology-aware placement decisions.
pub struct GpuTopologyMap {
    /// All known GPU devices, indexed by device ID
    devices: HashMap<String, GpuDevice>,
    /// Interconnect links (keyed as "from:to")
    links: HashMap<String, TopologyLink>,
}

impl GpuTopologyMap {
    /// Create an empty topology map
    pub fn new() -> Self {
        Self {
            devices: HashMap::new(),
            links: HashMap::new(),
        }
    }

    /// Register a GPU device
    pub fn add_device(&mut self, device: GpuDevice) {
        self.devices.insert(device.id.clone(), device);
    }

    /// Register an interconnect link between two GPUs (bidirectional)
    pub fn add_link(
        &mut self,
        from: &str,
        to: &str,
        interconnect: GpuInterconnect,
        link_count: u32,
    ) {
        let fwd = TopologyLink {
            from: from.to_string(),
            to: to.to_string(),
            interconnect,
            link_count,
        };
        let rev = TopologyLink {
            from: to.to_string(),
            to: from.to_string(),
            interconnect,
            link_count,
        };
        self.links.insert(format!("{from}:{to}"), fwd);
        self.links.insert(format!("{to}:{from}"), rev);
    }

    /// Get all devices on a specific host
    pub fn devices_on_host(&self, host_id: &str) -> Vec<&GpuDevice> {
        self.devices
            .values()
            .filter(|d| d.host_id == host_id && !d.allocated)
            .collect()
    }

    /// Get the interconnect between two devices
    pub fn link_between(&self, a: &str, b: &str) -> Option<&TopologyLink> {
        self.links.get(&format!("{a}:{b}"))
    }

    /// Get all unique host IDs
    pub fn hosts(&self) -> Vec<String> {
        let mut hosts: Vec<String> = self.devices.values().map(|d| d.host_id.clone()).collect();
        hosts.sort();
        hosts.dedup();
        hosts
    }

    /// Total GPU count
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// Available (unallocated) GPU count
    pub fn available_count(&self) -> usize {
        self.devices.values().filter(|d| !d.allocated).count()
    }

    /// Mark GPUs as allocated
    pub fn allocate(&mut self, gpu_ids: &[String]) {
        for id in gpu_ids {
            if let Some(dev) = self.devices.get_mut(id) {
                dev.allocated = true;
            }
        }
    }

    /// Release GPUs
    pub fn release(&mut self, gpu_ids: &[String]) {
        for id in gpu_ids {
            if let Some(dev) = self.devices.get_mut(id) {
                dev.allocated = false;
                dev.utilization = 0.0;
            }
        }
    }

    /// Find the best GPU placement for a workload
    ///
    /// Strategy:
    /// 1. Group available GPUs by host
    /// 2. Filter hosts that have enough GPUs meeting VRAM/CC requirements
    /// 3. For each candidate host, pick the best GPU subset
    /// 4. Score each subset on interconnect affinity and NUMA locality
    /// 5. Return the highest-scoring placement
    pub fn find_placement(&self, req: &GpuRequirements) -> TopologyResult<GpuPlacement> {
        let available: Vec<&GpuDevice> = self
            .devices
            .values()
            .filter(|d| !d.allocated)
            .filter(|d| d.vram_bytes >= req.min_vram_bytes)
            .filter(|d| d.compute_capability >= req.min_compute_capability)
            .collect();

        if (available.len() as u32) < req.gpu_count {
            return Err(TopologyError::InsufficientGpus {
                needed: req.gpu_count,
                available: available.len() as u32,
            });
        }

        // Group by host
        let mut by_host: HashMap<&str, Vec<&GpuDevice>> = HashMap::new();
        for dev in &available {
            by_host.entry(&dev.host_id).or_default().push(dev);
        }

        let mut best_placement: Option<GpuPlacement> = None;

        for (host_id, gpus) in &by_host {
            if (gpus.len() as u32) < req.gpu_count {
                continue;
            }

            // Try to find the best subset on this host
            if let Some(placement) = self.best_subset_on_host(host_id, gpus, req) {
                if best_placement
                    .as_ref()
                    .map_or(true, |b| placement.affinity_score > b.affinity_score)
                {
                    best_placement = Some(placement);
                }
            }
        }

        // If co-location not required and nothing found per-host, try cross-host
        if best_placement.is_none() && !req.prefer_colocated && !req.same_numa {
            if let Some(placement) = self.cross_host_placement(&available, req) {
                best_placement = Some(placement);
            }
        }

        best_placement.ok_or_else(|| {
            TopologyError::InterconnectUnsatisfied(format!(
                "No group of {} GPUs satisfies requirements",
                req.gpu_count
            ))
        })
    }

    /// Find the best subset of GPUs on a single host
    fn best_subset_on_host(
        &self,
        host_id: &str,
        gpus: &[&GpuDevice],
        req: &GpuRequirements,
    ) -> Option<GpuPlacement> {
        let count = req.gpu_count as usize;
        if gpus.len() < count {
            return None;
        }

        // For small GPU counts, enumerate combinations; for large, use greedy
        let subsets: Vec<Vec<&GpuDevice>> = if count <= 8 && gpus.len() <= 16 {
            combinations(gpus, count)
        } else {
            // Greedy: sort by NUMA node, pick first N
            let mut sorted: Vec<&GpuDevice> = gpus.to_vec();
            sorted.sort_by_key(|g| g.numa_node);
            vec![sorted.into_iter().take(count).collect()]
        };

        let mut best: Option<GpuPlacement> = None;

        for subset in &subsets {
            // Check NUMA constraint
            if req.same_numa {
                let numa = subset[0].numa_node;
                if !subset.iter().all(|g| g.numa_node == numa) {
                    continue;
                }
            }

            // Score interconnect
            let (score, bandwidth) = self.score_subset(subset);

            // Check NVLink requirement
            if req.require_nvlink && score < 0.85 {
                continue;
            }

            // Check bandwidth requirement
            if bandwidth < req.min_bandwidth_gbps {
                continue;
            }

            let same_numa = subset.windows(2).all(|w| w[0].numa_node == w[1].numa_node);

            let placement = GpuPlacement {
                gpu_ids: subset.iter().map(|g| g.id.clone()).collect(),
                host_id: host_id.to_string(),
                affinity_score: score,
                aggregate_bandwidth_gbps: bandwidth,
                same_numa,
            };

            if best.as_ref().map_or(true, |b| score > b.affinity_score) {
                best = Some(placement);
            }
        }

        best
    }

    /// Score a subset of GPUs based on their pairwise interconnect
    fn score_subset(&self, gpus: &[&GpuDevice]) -> (f64, f64) {
        if gpus.len() <= 1 {
            return (1.0, f64::INFINITY);
        }

        let mut total_score = 0.0;
        let mut min_bandwidth = f64::INFINITY;
        let mut pair_count = 0u32;

        for i in 0..gpus.len() {
            for j in (i + 1)..gpus.len() {
                let link = self.link_between(&gpus[i].id, &gpus[j].id);
                let (score, bw) = match link {
                    Some(l) => (
                        l.interconnect.affinity_score(),
                        l.interconnect.bandwidth_gbps() * l.link_count as f64,
                    ),
                    None => {
                        // No explicit link — infer from NUMA
                        if gpus[i].numa_node == gpus[j].numa_node {
                            (
                                GpuInterconnect::PcieRoot.affinity_score(),
                                GpuInterconnect::PcieRoot.bandwidth_gbps(),
                            )
                        } else {
                            (
                                GpuInterconnect::CrossNuma.affinity_score(),
                                GpuInterconnect::CrossNuma.bandwidth_gbps(),
                            )
                        }
                    }
                };
                total_score += score;
                min_bandwidth = min_bandwidth.min(bw);
                pair_count += 1;
            }
        }

        let avg_score = if pair_count > 0 {
            total_score / pair_count as f64
        } else {
            1.0
        };

        (avg_score, min_bandwidth)
    }

    /// Cross-host placement (when co-location is not required)
    fn cross_host_placement(
        &self,
        available: &[&GpuDevice],
        req: &GpuRequirements,
    ) -> Option<GpuPlacement> {
        // Simple greedy: pick the first N available GPUs, prefer same host
        let count = req.gpu_count as usize;
        if available.len() < count {
            return None;
        }

        let selected: Vec<String> = available[..count].iter().map(|g| g.id.clone()).collect();
        let host_id = available[0].host_id.clone();

        Some(GpuPlacement {
            gpu_ids: selected,
            host_id,
            affinity_score: 0.1, // Cross-host is low affinity
            aggregate_bandwidth_gbps: GpuInterconnect::CrossNuma.bandwidth_gbps(),
            same_numa: false,
        })
    }
}

impl Default for GpuTopologyMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate all combinations of `k` items from `items`
fn combinations<'a, T>(items: &[&'a T], k: usize) -> Vec<Vec<&'a T>> {
    if k == 0 {
        return vec![vec![]];
    }
    if items.len() < k {
        return vec![];
    }

    let mut result = Vec::new();
    for i in 0..=items.len() - k {
        for mut rest in combinations(&items[i + 1..], k - 1) {
            rest.insert(0, items[i]);
            result.push(rest);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dgx_topology() -> GpuTopologyMap {
        let mut map = GpuTopologyMap::new();
        // DGX-like: 8 GPUs, 2 NUMA nodes, NVSwitch fabric
        for i in 0..8 {
            let gpu = GpuDevice::new(format!("gpu-{i}"), "host-1", "A100-80GB")
                .numa(if i < 4 { 0 } else { 1 })
                .pci(format!("0000:{:02x}:00.0", i))
                .vram(80 * 1024 * 1024 * 1024)
                .capability(80);
            map.add_device(gpu);
        }
        // NVSwitch: all GPUs connected to all others
        for i in 0..8 {
            for j in (i + 1)..8 {
                let interconnect = if i / 4 == j / 4 {
                    GpuInterconnect::NvSwitch
                } else {
                    GpuInterconnect::NvLink
                };
                map.add_link(&format!("gpu-{i}"), &format!("gpu-{j}"), interconnect, 12);
            }
        }
        map
    }

    #[test]
    fn test_single_gpu_placement() {
        let map = dgx_topology();
        let req = GpuRequirements::default().count(1);
        let placement = map.find_placement(&req).unwrap();
        assert_eq!(placement.gpu_ids.len(), 1);
        assert_eq!(placement.host_id, "host-1");
        assert!(placement.affinity_score >= 0.9);
    }

    #[test]
    fn test_multi_gpu_nvlink() {
        let map = dgx_topology();
        let req = GpuRequirements::default().count(4).nvlink();
        let placement = map.find_placement(&req).unwrap();
        assert_eq!(placement.gpu_ids.len(), 4);
        assert!(placement.affinity_score >= 0.85);
    }

    #[test]
    fn test_same_numa_constraint() {
        let map = dgx_topology();
        let req = GpuRequirements::default().count(4).same_numa_node();
        let placement = map.find_placement(&req).unwrap();
        assert_eq!(placement.gpu_ids.len(), 4);
        assert!(placement.same_numa);
    }

    #[test]
    fn test_insufficient_gpus() {
        let map = dgx_topology();
        let req = GpuRequirements::default().count(16);
        let err = map.find_placement(&req).unwrap_err();
        assert!(matches!(err, TopologyError::InsufficientGpus { .. }));
    }

    #[test]
    fn test_vram_filter() {
        let map = dgx_topology();
        let req = GpuRequirements::default()
            .count(1)
            .min_vram(100 * 1024 * 1024 * 1024); // 100 GB — exceeds 80 GB
        let err = map.find_placement(&req).unwrap_err();
        assert!(matches!(err, TopologyError::InsufficientGpus { .. }));
    }

    #[test]
    fn test_allocate_and_release() {
        let mut map = dgx_topology();
        assert_eq!(map.available_count(), 8);

        let req = GpuRequirements::default().count(4);
        let placement = map.find_placement(&req).unwrap();
        map.allocate(&placement.gpu_ids);
        assert_eq!(map.available_count(), 4);

        // Second placement should still work with remaining 4
        let placement2 = map.find_placement(&req).unwrap();
        assert_eq!(placement2.gpu_ids.len(), 4);

        // Release first batch
        map.release(&placement.gpu_ids);
        assert_eq!(map.available_count(), 8);
    }

    #[test]
    fn test_interconnect_scores() {
        assert!(
            GpuInterconnect::NvSwitch.affinity_score() > GpuInterconnect::NvLink.affinity_score()
        );
        assert!(
            GpuInterconnect::NvLink.affinity_score() > GpuInterconnect::PciePeer.affinity_score()
        );
        assert!(
            GpuInterconnect::PciePeer.affinity_score() > GpuInterconnect::PcieRoot.affinity_score()
        );
        assert!(
            GpuInterconnect::PcieRoot.affinity_score()
                > GpuInterconnect::CrossNuma.affinity_score()
        );
    }

    #[test]
    fn test_bandwidth_ordering() {
        assert!(
            GpuInterconnect::NvSwitch.bandwidth_gbps() > GpuInterconnect::NvLink.bandwidth_gbps()
        );
        assert!(
            GpuInterconnect::NvLink.bandwidth_gbps() > GpuInterconnect::PciePeer.bandwidth_gbps()
        );
    }

    #[test]
    fn test_empty_map() {
        let map = GpuTopologyMap::new();
        assert_eq!(map.device_count(), 0);
        assert_eq!(map.available_count(), 0);
        let req = GpuRequirements::default().count(1);
        let err = map.find_placement(&req).unwrap_err();
        assert!(matches!(err, TopologyError::InsufficientGpus { .. }));
    }

    #[test]
    fn test_multi_host_topology() {
        let mut map = GpuTopologyMap::new();
        // Two hosts, 2 GPUs each
        for i in 0..2 {
            map.add_device(GpuDevice::new(format!("gpu-h1-{i}"), "host-1", "H100"));
            map.add_device(GpuDevice::new(format!("gpu-h2-{i}"), "host-2", "H100"));
        }
        map.add_link("gpu-h1-0", "gpu-h1-1", GpuInterconnect::NvLink, 4);
        map.add_link("gpu-h2-0", "gpu-h2-1", GpuInterconnect::NvLink, 4);

        let req = GpuRequirements::default().count(2);
        let placement = map.find_placement(&req).unwrap();
        assert_eq!(placement.gpu_ids.len(), 2);
        // Should pick GPUs on the same host
        let hosts: Vec<String> = placement
            .gpu_ids
            .iter()
            .filter_map(|id| map.devices.get(id))
            .map(|d| d.host_id.clone())
            .collect();
        assert_eq!(hosts[0], hosts[1]);
    }

    #[test]
    fn test_devices_on_host() {
        let map = dgx_topology();
        let devs = map.devices_on_host("host-1");
        assert_eq!(devs.len(), 8);
        let devs = map.devices_on_host("nonexistent");
        assert!(devs.is_empty());
    }

    #[test]
    fn test_hosts_list() {
        let map = dgx_topology();
        let hosts = map.hosts();
        assert_eq!(hosts, vec!["host-1"]);
    }

    #[test]
    fn test_link_lookup() {
        let map = dgx_topology();
        let link = map.link_between("gpu-0", "gpu-1").unwrap();
        assert_eq!(link.interconnect, GpuInterconnect::NvSwitch);
        assert_eq!(link.link_count, 12);
        // Reverse direction
        let rev = map.link_between("gpu-1", "gpu-0").unwrap();
        assert_eq!(rev.interconnect, GpuInterconnect::NvSwitch);
    }
}
