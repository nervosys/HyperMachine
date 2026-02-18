//! Profiler Implementation
//!
//! Provides CPU profiling, flame graph generation, and hot path detection
//! for performance analysis of the hypervisor.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// Profile entry representing a sampled stack frame
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProfileFrame {
    /// Function or symbol name
    pub name: String,
    /// Module or file name
    pub module: String,
    /// Optional line number
    pub line: Option<u32>,
    /// Instruction address
    pub address: u64,
}

impl ProfileFrame {
    /// Create a new frame
    pub fn new(name: impl Into<String>, module: impl Into<String>, address: u64) -> Self {
        Self {
            name: name.into(),
            module: module.into(),
            line: None,
            address,
        }
    }

    /// Add line number
    pub fn with_line(mut self, line: u32) -> Self {
        self.line = Some(line);
        self
    }
}

/// Stack sample containing a complete call stack
#[derive(Debug, Clone)]
pub struct StackSample {
    /// Stack frames from bottom (root) to top (current)
    pub frames: Vec<ProfileFrame>,
    /// Timestamp when sample was taken
    pub timestamp: Instant,
    /// Thread ID
    pub thread_id: u64,
    /// CPU this was sampled on
    pub cpu_id: Option<u32>,
    /// Sample weight (e.g., CPU cycles, time)
    pub weight: u64,
}

impl StackSample {
    /// Create a new sample
    pub fn new(frames: Vec<ProfileFrame>, thread_id: u64) -> Self {
        Self {
            frames,
            timestamp: Instant::now(),
            thread_id,
            cpu_id: None,
            weight: 1,
        }
    }

    /// Set sample weight
    pub fn with_weight(mut self, weight: u64) -> Self {
        self.weight = weight;
        self
    }

    /// Set CPU ID
    pub fn with_cpu(mut self, cpu_id: u32) -> Self {
        self.cpu_id = Some(cpu_id);
        self
    }

    /// Get the leaf (top) frame
    pub fn leaf(&self) -> Option<&ProfileFrame> {
        self.frames.last()
    }

    /// Get the root (bottom) frame
    pub fn root(&self) -> Option<&ProfileFrame> {
        self.frames.first()
    }
}

/// Aggregated profile data
#[derive(Debug, Clone)]
pub struct ProfileData {
    /// Aggregated stack counts
    stacks: HashMap<Vec<String>, u64>,
    /// Total samples
    total_samples: u64,
    /// Total weight
    total_weight: u64,
    /// Profile duration
    duration: Duration,
    /// Start time
    start_time: Instant,
}

impl ProfileData {
    /// Create empty profile data
    fn new() -> Self {
        Self {
            stacks: HashMap::new(),
            total_samples: 0,
            total_weight: 0,
            duration: Duration::ZERO,
            start_time: Instant::now(),
        }
    }

    /// Add a sample
    fn add_sample(&mut self, sample: &StackSample) {
        let stack: Vec<String> = sample.frames.iter().map(|f| f.name.clone()).collect();

        *self.stacks.entry(stack).or_insert(0) += sample.weight;
        self.total_samples += 1;
        self.total_weight += sample.weight;
    }

    /// Get top functions by sample count
    pub fn top_functions(&self, limit: usize) -> Vec<(String, u64, f64)> {
        let mut func_counts: HashMap<String, u64> = HashMap::new();

        for (stack, count) in &self.stacks {
            for func in stack {
                *func_counts.entry(func.clone()).or_insert(0) += count;
            }
        }

        let mut sorted: Vec<_> = func_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));

        sorted
            .into_iter()
            .take(limit)
            .map(|(name, count)| {
                let pct = if self.total_weight > 0 {
                    (count as f64 / self.total_weight as f64) * 100.0
                } else {
                    0.0
                };
                (name, count, pct)
            })
            .collect()
    }

    /// Get hot paths (most sampled complete stacks)
    pub fn hot_paths(&self, limit: usize) -> Vec<(Vec<String>, u64, f64)> {
        let mut sorted: Vec<_> = self.stacks.iter().map(|(k, v)| (k.clone(), *v)).collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));

        sorted
            .into_iter()
            .take(limit)
            .map(|(stack, count)| {
                let pct = if self.total_weight > 0 {
                    (count as f64 / self.total_weight as f64) * 100.0
                } else {
                    0.0
                };
                (stack, count, pct)
            })
            .collect()
    }

    /// Generate flame graph data in folded format
    pub fn to_folded_format(&self) -> String {
        let mut lines: Vec<String> = Vec::new();

        for (stack, count) in &self.stacks {
            if !stack.is_empty() {
                let line = format!("{} {}", stack.join(";"), count);
                lines.push(line);
            }
        }

        lines.sort();
        lines.join("\n")
    }

    /// Get total sample count
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    /// Get total weight
    pub fn total_weight(&self) -> u64 {
        self.total_weight
    }

    /// Get profile duration
    pub fn duration(&self) -> Duration {
        self.duration
    }
}

/// Profiler error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProfilerError {
    /// Profiler not started
    #[error("profiler not started")]
    NotStarted,
    /// Profiler already running
    #[error("profiler already running")]
    AlreadyRunning,
    /// Lock acquisition failed
    #[error("lock acquisition failed")]
    LockError,
    /// Invalid configuration
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

/// Profiler result type
pub type ProfilerResult<T> = Result<T, ProfilerError>;

/// Profiler configuration
#[derive(Debug, Clone)]
pub struct ProfilerConfig {
    /// Sampling frequency in Hz
    pub frequency: u32,
    /// Maximum samples to collect
    pub max_samples: usize,
    /// Whether to include kernel frames
    pub include_kernel: bool,
    /// Maximum stack depth
    pub max_stack_depth: usize,
}

impl Default for ProfilerConfig {
    fn default() -> Self {
        Self {
            frequency: 99, // Avoid lockstep with timers
            max_samples: 100_000,
            include_kernel: false,
            max_stack_depth: 128,
        }
    }
}

impl ProfilerConfig {
    /// Set sampling frequency
    pub fn with_frequency(mut self, hz: u32) -> Self {
        self.frequency = hz;
        self
    }

    /// Set max samples
    pub fn with_max_samples(mut self, max: usize) -> Self {
        self.max_samples = max;
        self
    }

    /// Include kernel frames
    pub fn with_kernel(mut self, include: bool) -> Self {
        self.include_kernel = include;
        self
    }
}

/// Profiler state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfilerState {
    /// Profiler is idle
    Idle,
    /// Profiler is running
    Running,
    /// Profiler is paused
    Paused,
}

/// CPU profiler
pub struct CpuProfiler {
    /// Configuration
    config: ProfilerConfig,
    /// Current state
    state: RwLock<ProfilerState>,
    /// Collected samples
    samples: Mutex<Vec<StackSample>>,
    /// Start time
    start_time: Mutex<Option<Instant>>,
}

impl CpuProfiler {
    /// Create a new profiler with default config
    pub fn new() -> Self {
        Self::with_config(ProfilerConfig::default())
    }

    /// Create with custom config
    pub fn with_config(config: ProfilerConfig) -> Self {
        Self {
            config,
            state: RwLock::new(ProfilerState::Idle),
            samples: Mutex::new(Vec::new()),
            start_time: Mutex::new(None),
        }
    }

    /// Get current state
    pub fn state(&self) -> ProfilerState {
        self.state.read().map(|s| *s).unwrap_or(ProfilerState::Idle)
    }

    /// Start profiling
    pub fn start(&self) -> ProfilerResult<()> {
        let mut state = self.state.write().map_err(|_| ProfilerError::LockError)?;
        if *state == ProfilerState::Running {
            return Err(ProfilerError::AlreadyRunning);
        }

        let mut samples = self.samples.lock().map_err(|_| ProfilerError::LockError)?;
        samples.clear();

        let mut start_time = self
            .start_time
            .lock()
            .map_err(|_| ProfilerError::LockError)?;
        *start_time = Some(Instant::now());

        *state = ProfilerState::Running;
        Ok(())
    }

    /// Stop profiling and return data
    pub fn stop(&self) -> ProfilerResult<ProfileData> {
        let mut state = self.state.write().map_err(|_| ProfilerError::LockError)?;
        if *state == ProfilerState::Idle {
            return Err(ProfilerError::NotStarted);
        }

        *state = ProfilerState::Idle;

        let samples = self.samples.lock().map_err(|_| ProfilerError::LockError)?;
        let start_time = self
            .start_time
            .lock()
            .map_err(|_| ProfilerError::LockError)?;

        let mut data = ProfileData::new();
        if let Some(start) = *start_time {
            data.start_time = start;
            data.duration = start.elapsed();
        }

        for sample in samples.iter() {
            data.add_sample(sample);
        }

        Ok(data)
    }

    /// Pause profiling
    pub fn pause(&self) -> ProfilerResult<()> {
        let mut state = self.state.write().map_err(|_| ProfilerError::LockError)?;
        if *state != ProfilerState::Running {
            return Err(ProfilerError::NotStarted);
        }
        *state = ProfilerState::Paused;
        Ok(())
    }

    /// Resume profiling
    pub fn resume(&self) -> ProfilerResult<()> {
        let mut state = self.state.write().map_err(|_| ProfilerError::LockError)?;
        if *state != ProfilerState::Paused {
            return Err(ProfilerError::NotStarted);
        }
        *state = ProfilerState::Running;
        Ok(())
    }

    /// Add a sample (called by sampling mechanism)
    pub fn add_sample(&self, sample: StackSample) -> ProfilerResult<()> {
        let state = self.state.read().map_err(|_| ProfilerError::LockError)?;
        if *state != ProfilerState::Running {
            return Ok(()); // Silently ignore if not running
        }
        drop(state);

        let mut samples = self.samples.lock().map_err(|_| ProfilerError::LockError)?;
        if samples.len() < self.config.max_samples {
            samples.push(sample);
        }
        Ok(())
    }

    /// Get sample count
    pub fn sample_count(&self) -> usize {
        self.samples.lock().map(|s| s.len()).unwrap_or(0)
    }

    /// Get configuration
    pub fn config(&self) -> &ProfilerConfig {
        &self.config
    }
}

impl Default for CpuProfiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Flame graph node
#[derive(Debug, Clone)]
pub struct FlameGraphNode {
    /// Function name
    pub name: String,
    /// Self time/count (time spent in this function, not children)
    pub self_value: u64,
    /// Total time/count (including children)
    pub total_value: u64,
    /// Child nodes
    pub children: Vec<FlameGraphNode>,
}

impl FlameGraphNode {
    /// Create a new node
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            self_value: 0,
            total_value: 0,
            children: Vec::new(),
        }
    }

    /// Find or create a child with the given name
    fn get_or_create_child(&mut self, name: &str) -> &mut FlameGraphNode {
        if let Some(pos) = self.children.iter().position(|c| c.name == name) {
            &mut self.children[pos]
        } else {
            self.children.push(FlameGraphNode::new(name));
            self.children.last_mut().expect("just pushed a child node")
        }
    }
}

/// Flame graph builder
pub struct FlameGraphBuilder {
    root: FlameGraphNode,
}

impl FlameGraphBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            root: FlameGraphNode::new("root"),
        }
    }

    /// Add a sample to the flame graph
    pub fn add_sample(&mut self, stack: &[String], value: u64) {
        if stack.is_empty() {
            return;
        }

        let mut current = &mut self.root;
        current.total_value += value;

        for (i, frame) in stack.iter().enumerate() {
            current = current.get_or_create_child(frame);
            current.total_value += value;

            // Last frame gets self time
            if i == stack.len() - 1 {
                current.self_value += value;
            }
        }
    }

    /// Build from profile data
    pub fn from_profile(data: &ProfileData) -> Self {
        let mut builder = Self::new();

        for (stack, count) in &data.stacks {
            builder.add_sample(stack, *count);
        }

        builder
    }

    /// Get the root node
    pub fn root(&self) -> &FlameGraphNode {
        &self.root
    }

    /// Build into a root node
    pub fn build(self) -> FlameGraphNode {
        self.root
    }

    /// Generate SVG flame graph
    pub fn to_svg(&self, width: u32, height: u32) -> String {
        let mut svg = String::new();

        svg.push_str(&format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
            width, height, width, height
        ));
        svg.push_str(
            r#"<style>
            .frame:hover { stroke: black; stroke-width: 1px; cursor: pointer; }
            .frame text { font-family: monospace; font-size: 12px; fill: white; }
        </style>"#,
        );

        // Render frames recursively
        if self.root.total_value > 0 {
            self.render_node_svg(
                &mut svg,
                &self.root,
                0.0,
                width as f64,
                height as f64 - 20.0,
                height as f64 / 20.0,
            );
        }

        svg.push_str("</svg>");
        svg
    }

    fn render_node_svg(
        &self,
        svg: &mut String,
        node: &FlameGraphNode,
        x: f64,
        width: f64,
        y: f64,
        height: f64,
    ) {
        if width < 1.0 || node.name == "root" {
            // Don't render root, just process children
            let mut child_x = x;
            for child in &node.children {
                let child_width = (child.total_value as f64 / node.total_value as f64) * width;
                self.render_node_svg(svg, child, child_x, child_width, y - height, height);
                child_x += child_width;
            }
            return;
        }

        // Generate color based on name hash
        let color = self.name_to_color(&node.name);

        svg.push_str(&format!(
            r#"<g class="frame"><rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{}"/>"#,
            x, y, width.max(1.0), height - 1.0, color
        ));

        if width > 30.0 {
            let text_width = (width - 6.0) / 7.0; // Approximate char width
            let display_name: String = node.name.chars().take(text_width as usize).collect();
            svg.push_str(&format!(
                r#"<text x="{:.1}" y="{:.1}">{}</text>"#,
                x + 3.0,
                y + height - 5.0,
                display_name
            ));
        }

        svg.push_str("</g>");

        // Render children
        let mut child_x = x;
        for child in &node.children {
            let child_width = (child.total_value as f64 / node.total_value as f64) * width;
            self.render_node_svg(svg, child, child_x, child_width, y - height, height);
            child_x += child_width;
        }
    }

    fn name_to_color(&self, name: &str) -> String {
        // Simple hash-based coloring
        let hash: u32 = name
            .bytes()
            .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));

        // Warm color palette (reds, oranges, yellows)
        let r = 200 + (hash % 55) as u8;
        let g = 50 + ((hash >> 8) % 150) as u8;
        let b = 20 + ((hash >> 16) % 60) as u8;

        format!("rgb({},{},{})", r, g, b)
    }
}

impl Default for FlameGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Function-level profile with call counts and timing
#[derive(Debug, Clone)]
pub struct FunctionProfile {
    /// Function name
    pub name: String,
    /// Total call count
    pub call_count: u64,
    /// Total time in nanoseconds
    pub total_time_ns: u64,
    /// Self time in nanoseconds (excluding callees)
    pub self_time_ns: u64,
    /// Minimum call duration
    pub min_time_ns: u64,
    /// Maximum call duration
    pub max_time_ns: u64,
}

impl FunctionProfile {
    /// Create a new function profile
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            call_count: 0,
            total_time_ns: 0,
            self_time_ns: 0,
            min_time_ns: u64::MAX,
            max_time_ns: 0,
        }
    }

    /// Average call time
    pub fn avg_time_ns(&self) -> u64 {
        if self.call_count > 0 {
            self.total_time_ns / self.call_count
        } else {
            0
        }
    }

    /// Record a call
    fn record(&mut self, duration_ns: u64) {
        self.call_count += 1;
        self.total_time_ns += duration_ns;
        self.min_time_ns = self.min_time_ns.min(duration_ns);
        self.max_time_ns = self.max_time_ns.max(duration_ns);
    }
}

/// Instrumentation-based profiler for precise timing
pub struct InstrumentedProfiler {
    /// Function profiles
    profiles: RwLock<HashMap<String, FunctionProfile>>,
    /// Active spans (for nested calls)
    active_spans: Mutex<HashMap<u64, Vec<(String, Instant)>>>,
    /// Next span ID
    next_span_id: std::sync::atomic::AtomicU64,
}

impl InstrumentedProfiler {
    /// Create a new profiler
    pub fn new() -> Self {
        Self {
            profiles: RwLock::new(HashMap::new()),
            active_spans: Mutex::new(HashMap::new()),
            next_span_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Enter a function
    pub fn enter(&self, name: impl Into<String>) -> ProfileGuard {
        let name = name.into();
        let thread_id = std::thread::current().id();
        let thread_hash = format!("{:?}", thread_id).len() as u64; // Simple thread identifier

        if let Ok(mut active) = self.active_spans.lock() {
            active
                .entry(thread_hash)
                .or_insert_with(Vec::new)
                .push((name.clone(), Instant::now()));
        }

        ProfileGuard {
            profiler: self,
            name,
            start: Instant::now(),
            thread_hash,
        }
    }

    /// Get all function profiles
    pub fn get_profiles(&self) -> Vec<FunctionProfile> {
        self.profiles
            .read()
            .map(|p| p.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get top functions by total time
    pub fn top_by_total_time(&self, limit: usize) -> Vec<FunctionProfile> {
        let mut profiles = self.get_profiles();
        profiles.sort_by(|a, b| b.total_time_ns.cmp(&a.total_time_ns));
        profiles.into_iter().take(limit).collect()
    }

    /// Get top functions by call count
    pub fn top_by_call_count(&self, limit: usize) -> Vec<FunctionProfile> {
        let mut profiles = self.get_profiles();
        profiles.sort_by(|a, b| b.call_count.cmp(&a.call_count));
        profiles.into_iter().take(limit).collect()
    }

    /// Reset all profiles
    pub fn reset(&self) {
        if let Ok(mut profiles) = self.profiles.write() {
            profiles.clear();
        }
        if let Ok(mut active) = self.active_spans.lock() {
            active.clear();
        }
    }

    fn record_exit(&self, name: &str, duration_ns: u64, thread_hash: u64) {
        // Remove from active spans
        if let Ok(mut active) = self.active_spans.lock() {
            if let Some(stack) = active.get_mut(&thread_hash) {
                stack.pop();
            }
        }

        // Record the call
        if let Ok(mut profiles) = self.profiles.write() {
            profiles
                .entry(name.to_string())
                .or_insert_with(|| FunctionProfile::new(name))
                .record(duration_ns);
        }
    }
}

impl Default for InstrumentedProfiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Guard that records function exit
pub struct ProfileGuard<'a> {
    profiler: &'a InstrumentedProfiler,
    name: String,
    start: Instant,
    thread_hash: u64,
}

impl<'a> Drop for ProfileGuard<'a> {
    fn drop(&mut self) {
        let duration = self.start.elapsed().as_nanos() as u64;
        self.profiler
            .record_exit(&self.name, duration, self.thread_hash);
    }
}

/// Memory allocation profiler
#[derive(Debug)]
pub struct AllocationProfile {
    /// Total allocations
    pub total_allocations: u64,
    /// Total bytes allocated
    pub total_bytes: u64,
    /// Current live allocations
    pub live_allocations: u64,
    /// Current live bytes
    pub live_bytes: u64,
    /// Peak live bytes
    pub peak_bytes: u64,
    /// Allocation site counts
    pub sites: HashMap<String, AllocationSite>,
}

/// Allocation site data
#[derive(Debug, Clone)]
pub struct AllocationSite {
    /// Call site identifier
    pub name: String,
    /// Number of allocations
    pub count: u64,
    /// Total bytes allocated
    pub bytes: u64,
}

impl AllocationProfile {
    /// Create empty profile
    pub fn new() -> Self {
        Self {
            total_allocations: 0,
            total_bytes: 0,
            live_allocations: 0,
            live_bytes: 0,
            peak_bytes: 0,
            sites: HashMap::new(),
        }
    }

    /// Record an allocation
    pub fn record_alloc(&mut self, size: u64, site: impl Into<String>) {
        self.total_allocations += 1;
        self.total_bytes += size;
        self.live_allocations += 1;
        self.live_bytes += size;
        self.peak_bytes = self.peak_bytes.max(self.live_bytes);

        let site = site.into();
        let entry = self.sites.entry(site.clone()).or_insert(AllocationSite {
            name: site,
            count: 0,
            bytes: 0,
        });
        entry.count += 1;
        entry.bytes += size;
    }

    /// Record a deallocation
    pub fn record_free(&mut self, size: u64) {
        self.live_allocations = self.live_allocations.saturating_sub(1);
        self.live_bytes = self.live_bytes.saturating_sub(size);
    }

    /// Get top allocation sites by bytes
    pub fn top_sites(&self, limit: usize) -> Vec<&AllocationSite> {
        let mut sites: Vec<_> = self.sites.values().collect();
        sites.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        sites.into_iter().take(limit).collect()
    }
}

impl Default for AllocationProfile {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_frame() {
        let frame = ProfileFrame::new("my_func", "my_module", 0x1234).with_line(42);

        assert_eq!(frame.name, "my_func");
        assert_eq!(frame.module, "my_module");
        assert_eq!(frame.line, Some(42));
        assert_eq!(frame.address, 0x1234);
    }

    #[test]
    fn test_stack_sample() {
        let frames = vec![
            ProfileFrame::new("main", "app", 0x1000),
            ProfileFrame::new("foo", "app", 0x2000),
            ProfileFrame::new("bar", "app", 0x3000),
        ];

        let sample = StackSample::new(frames, 1234).with_weight(100).with_cpu(0);

        assert_eq!(sample.leaf().unwrap().name, "bar");
        assert_eq!(sample.root().unwrap().name, "main");
        assert_eq!(sample.weight, 100);
    }

    #[test]
    fn test_profile_data_aggregation() {
        let mut data = ProfileData::new();

        let frames1 = vec![
            ProfileFrame::new("main", "app", 0),
            ProfileFrame::new("foo", "app", 0),
        ];
        data.add_sample(&StackSample::new(frames1, 0).with_weight(10));

        let frames2 = vec![
            ProfileFrame::new("main", "app", 0),
            ProfileFrame::new("bar", "app", 0),
        ];
        data.add_sample(&StackSample::new(frames2, 0).with_weight(20));

        assert_eq!(data.total_samples(), 2);
        assert_eq!(data.total_weight(), 30);

        let top = data.top_functions(10);
        assert!(top.iter().any(|(name, _, _)| name == "main"));
    }

    #[test]
    fn test_folded_format() {
        let mut data = ProfileData::new();

        let frames = vec![
            ProfileFrame::new("main", "app", 0),
            ProfileFrame::new("foo", "app", 0),
        ];
        data.add_sample(&StackSample::new(frames.clone(), 0).with_weight(5));
        data.add_sample(&StackSample::new(frames, 0).with_weight(5));

        let folded = data.to_folded_format();
        assert!(folded.contains("main;foo 10"));
    }

    #[test]
    fn test_cpu_profiler_lifecycle() {
        let profiler = CpuProfiler::new();

        assert_eq!(profiler.state(), ProfilerState::Idle);

        profiler.start().unwrap();
        assert_eq!(profiler.state(), ProfilerState::Running);

        // Can't start again
        assert!(profiler.start().is_err());

        profiler.pause().unwrap();
        assert_eq!(profiler.state(), ProfilerState::Paused);

        profiler.resume().unwrap();
        assert_eq!(profiler.state(), ProfilerState::Running);

        let data = profiler.stop().unwrap();
        assert_eq!(profiler.state(), ProfilerState::Idle);
        assert_eq!(data.total_samples(), 0);
    }

    #[test]
    fn test_profiler_sampling() {
        let profiler = CpuProfiler::new();
        profiler.start().unwrap();

        let frames = vec![
            ProfileFrame::new("main", "app", 0),
            ProfileFrame::new("compute", "app", 0),
        ];
        profiler.add_sample(StackSample::new(frames, 1)).unwrap();

        assert_eq!(profiler.sample_count(), 1);

        let data = profiler.stop().unwrap();
        assert_eq!(data.total_samples(), 1);
    }

    #[test]
    fn test_flame_graph_builder() {
        let mut builder = FlameGraphBuilder::new();

        builder.add_sample(&["main".to_string(), "foo".to_string()], 10);
        builder.add_sample(&["main".to_string(), "bar".to_string()], 20);
        builder.add_sample(
            &["main".to_string(), "foo".to_string(), "baz".to_string()],
            5,
        );

        let root = builder.build();
        assert_eq!(root.total_value, 35);

        let main = &root.children[0];
        assert_eq!(main.name, "main");
        assert_eq!(main.total_value, 35);
    }

    #[test]
    fn test_flame_graph_from_profile() {
        let mut data = ProfileData::new();
        let frames = vec![
            ProfileFrame::new("main", "app", 0),
            ProfileFrame::new("process", "app", 0),
        ];
        data.add_sample(&StackSample::new(frames, 0).with_weight(100));

        let builder = FlameGraphBuilder::from_profile(&data);
        assert_eq!(builder.root().total_value, 100);
    }

    #[test]
    fn test_flame_graph_svg() {
        let mut builder = FlameGraphBuilder::new();
        builder.add_sample(&["main".to_string(), "foo".to_string()], 10);

        let svg = builder.to_svg(800, 400);
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("frame"));
    }

    #[test]
    fn test_instrumented_profiler() {
        let profiler = InstrumentedProfiler::new();

        {
            let _guard = profiler.enter("test_function");
            std::thread::sleep(Duration::from_millis(1));
        }

        let profiles = profiler.get_profiles();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "test_function");
        assert_eq!(profiles[0].call_count, 1);
        assert!(profiles[0].total_time_ns > 0);
    }

    #[test]
    fn test_instrumented_profiler_nested() {
        let profiler = InstrumentedProfiler::new();

        {
            let _outer = profiler.enter("outer");
            {
                let _inner = profiler.enter("inner");
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        let profiles = profiler.get_profiles();
        assert_eq!(profiles.len(), 2);
    }

    #[test]
    fn test_instrumented_profiler_top_functions() {
        let profiler = InstrumentedProfiler::new();

        for _ in 0..5 {
            let _guard = profiler.enter("frequent");
        }

        for _ in 0..2 {
            let _guard = profiler.enter("rare");
        }

        let top = profiler.top_by_call_count(1);
        assert_eq!(top[0].name, "frequent");
        assert_eq!(top[0].call_count, 5);
    }

    #[test]
    fn test_function_profile() {
        let mut profile = FunctionProfile::new("test");
        profile.record(100);
        profile.record(200);
        profile.record(150);

        assert_eq!(profile.call_count, 3);
        assert_eq!(profile.total_time_ns, 450);
        assert_eq!(profile.avg_time_ns(), 150);
        assert_eq!(profile.min_time_ns, 100);
        assert_eq!(profile.max_time_ns, 200);
    }

    #[test]
    fn test_allocation_profile() {
        let mut profile = AllocationProfile::new();

        profile.record_alloc(1024, "malloc");
        profile.record_alloc(2048, "malloc");
        profile.record_alloc(512, "realloc");

        assert_eq!(profile.total_allocations, 3);
        assert_eq!(profile.total_bytes, 3584);
        assert_eq!(profile.live_bytes, 3584);
        assert_eq!(profile.peak_bytes, 3584);

        profile.record_free(1024);
        assert_eq!(profile.live_bytes, 2560);
        assert_eq!(profile.peak_bytes, 3584); // Peak unchanged
    }

    #[test]
    fn test_allocation_top_sites() {
        let mut profile = AllocationProfile::new();

        profile.record_alloc(1000, "site_a");
        profile.record_alloc(5000, "site_b");
        profile.record_alloc(2000, "site_a");

        let top = profile.top_sites(2);
        assert_eq!(top[0].name, "site_b");
        assert_eq!(top[0].bytes, 5000);
        assert_eq!(top[1].name, "site_a");
        assert_eq!(top[1].bytes, 3000);
    }

    #[test]
    fn test_profiler_config() {
        let config = ProfilerConfig::default()
            .with_frequency(199)
            .with_max_samples(50000)
            .with_kernel(true);

        assert_eq!(config.frequency, 199);
        assert_eq!(config.max_samples, 50000);
        assert!(config.include_kernel);
    }

    #[test]
    fn test_hot_paths() {
        let mut data = ProfileData::new();

        // Add same stack multiple times
        let hot_stack = vec![
            ProfileFrame::new("main", "app", 0),
            ProfileFrame::new("hot_function", "app", 0),
        ];
        for _ in 0..10 {
            data.add_sample(&StackSample::new(hot_stack.clone(), 0));
        }

        let cold_stack = vec![
            ProfileFrame::new("main", "app", 0),
            ProfileFrame::new("cold_function", "app", 0),
        ];
        data.add_sample(&StackSample::new(cold_stack, 0));

        let hot_paths = data.hot_paths(1);
        assert_eq!(hot_paths.len(), 1);
        assert!(hot_paths[0].0.contains(&"hot_function".to_string()));
    }

    #[test]
    fn test_profiler_reset() {
        let profiler = InstrumentedProfiler::new();

        {
            let _guard = profiler.enter("test");
        }

        assert_eq!(profiler.get_profiles().len(), 1);

        profiler.reset();
        assert_eq!(profiler.get_profiles().len(), 0);
    }

    #[test]
    fn test_profiler_error_display() {
        assert_eq!(
            format!("{}", ProfilerError::NotStarted),
            "profiler not started"
        );
        assert_eq!(
            format!("{}", ProfilerError::AlreadyRunning),
            "profiler already running"
        );
        assert_eq!(
            format!("{}", ProfilerError::LockError),
            "lock acquisition failed"
        );
        assert_eq!(
            format!("{}", ProfilerError::InvalidConfig("test".into())),
            "invalid config: test"
        );
    }
}
