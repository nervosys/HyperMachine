//! Display backend abstraction
//!
//! This module provides a trait-based abstraction for display output,
//! allowing different backends (null, memory, SDL, etc.) to be used
//! interchangeably.

use std::sync::{Arc, RwLock};

use super::framebuffer::{Color, Framebuffer, FramebufferConfig, PixelFormat, Rect};

/// Display backend trait
pub trait DisplayBackend: Send + Sync {
    /// Get the backend name
    fn name(&self) -> &str;

    /// Check if the display is connected/available
    fn is_connected(&self) -> bool;

    /// Get the current display resolution
    fn resolution(&self) -> (u32, u32);

    /// Set the display resolution (may fail if backend doesn't support it)
    fn set_resolution(&mut self, width: u32, height: u32) -> Result<(), DisplayError>;

    /// Update the display with framebuffer contents
    fn update(&mut self, framebuffer: &Framebuffer) -> Result<(), DisplayError>;

    /// Update a specific region of the display
    fn update_region(
        &mut self,
        framebuffer: &Framebuffer,
        region: Rect,
    ) -> Result<(), DisplayError>;

    /// Set window title (if applicable)
    fn set_title(&mut self, _title: &str) {}

    /// Show/hide cursor
    fn show_cursor(&mut self, _show: bool) {}

    /// Set cursor position
    fn set_cursor_position(&mut self, _x: u32, _y: u32) {}

    /// Flush any pending updates to the display
    fn flush(&mut self) -> Result<(), DisplayError> {
        Ok(())
    }
}

/// Display error types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayError {
    /// Display not connected
    NotConnected,
    /// Resolution not supported
    ResolutionNotSupported,
    /// Invalid framebuffer format
    InvalidFormat,
    /// Backend-specific error
    BackendError(String),
}

impl std::fmt::Display for DisplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisplayError::NotConnected => write!(f, "Display not connected"),
            DisplayError::ResolutionNotSupported => write!(f, "Resolution not supported"),
            DisplayError::InvalidFormat => write!(f, "Invalid framebuffer format"),
            DisplayError::BackendError(msg) => write!(f, "Backend error: {}", msg),
        }
    }
}

impl std::error::Error for DisplayError {}

/// Null display backend (discards all output)
#[derive(Debug)]
pub struct NullDisplayBackend {
    width: u32,
    height: u32,
    connected: bool,
}

impl NullDisplayBackend {
    /// Create a new null display backend
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            connected: true,
        }
    }

    /// Set connected state
    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
    }
}

impl Default for NullDisplayBackend {
    fn default() -> Self {
        Self::new(800, 600)
    }
}

impl DisplayBackend for NullDisplayBackend {
    fn name(&self) -> &str {
        "null"
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn resolution(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn set_resolution(&mut self, width: u32, height: u32) -> Result<(), DisplayError> {
        self.width = width;
        self.height = height;
        Ok(())
    }

    fn update(&mut self, _framebuffer: &Framebuffer) -> Result<(), DisplayError> {
        if !self.connected {
            return Err(DisplayError::NotConnected);
        }
        Ok(())
    }

    fn update_region(
        &mut self,
        _framebuffer: &Framebuffer,
        _region: Rect,
    ) -> Result<(), DisplayError> {
        if !self.connected {
            return Err(DisplayError::NotConnected);
        }
        Ok(())
    }
}

/// Memory display backend (stores output in memory)
#[derive(Debug)]
pub struct MemoryDisplayBackend {
    framebuffer: Framebuffer,
    connected: bool,
    update_count: u64,
    title: String,
    cursor_visible: bool,
    cursor_x: u32,
    cursor_y: u32,
}

impl MemoryDisplayBackend {
    /// Create a new memory display backend
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            framebuffer: Framebuffer::new(FramebufferConfig::new(width, height, PixelFormat::Xrgb32)),
            connected: true,
            update_count: 0,
            title: String::new(),
            cursor_visible: true,
            cursor_x: 0,
            cursor_y: 0,
        }
    }

    /// Get the internal framebuffer
    pub fn framebuffer(&self) -> &Framebuffer {
        &self.framebuffer
    }

    /// Get the update count
    pub fn update_count(&self) -> u64 {
        self.update_count
    }

    /// Get a pixel from the display
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<Color> {
        self.framebuffer.get_pixel(x, y)
    }

    /// Set connected state
    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
    }

    /// Get title
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Get cursor state
    pub fn cursor_state(&self) -> (bool, u32, u32) {
        (self.cursor_visible, self.cursor_x, self.cursor_y)
    }
}

impl Default for MemoryDisplayBackend {
    fn default() -> Self {
        Self::new(800, 600)
    }
}

impl DisplayBackend for MemoryDisplayBackend {
    fn name(&self) -> &str {
        "memory"
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn resolution(&self) -> (u32, u32) {
        (self.framebuffer.width(), self.framebuffer.height())
    }

    fn set_resolution(&mut self, width: u32, height: u32) -> Result<(), DisplayError> {
        self.framebuffer.resize(FramebufferConfig::new(width, height, PixelFormat::Xrgb32));
        Ok(())
    }

    fn update(&mut self, framebuffer: &Framebuffer) -> Result<(), DisplayError> {
        if !self.connected {
            return Err(DisplayError::NotConnected);
        }

        // Copy framebuffer data
        let src = framebuffer.buffer();
        let dst = self.framebuffer.buffer_mut();
        let len = src.len().min(dst.len());
        dst[..len].copy_from_slice(&src[..len]);

        self.update_count += 1;
        Ok(())
    }

    fn update_region(
        &mut self,
        framebuffer: &Framebuffer,
        region: Rect,
    ) -> Result<(), DisplayError> {
        if !self.connected {
            return Err(DisplayError::NotConnected);
        }

        // Copy region pixel by pixel
        for y in region.y..region.y + region.height {
            for x in region.x..region.x + region.width {
                if let Some(color) = framebuffer.get_pixel(x, y) {
                    self.framebuffer.set_pixel(x, y, color);
                }
            }
        }

        self.update_count += 1;
        Ok(())
    }

    fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
    }

    fn show_cursor(&mut self, show: bool) {
        self.cursor_visible = show;
    }

    fn set_cursor_position(&mut self, x: u32, y: u32) {
        self.cursor_x = x;
        self.cursor_y = y;
    }
}

/// Callback-based display backend
pub struct CallbackDisplayBackend {
    width: u32,
    height: u32,
    connected: bool,
    on_update: Box<dyn Fn(&Framebuffer) + Send + Sync>,
}

impl CallbackDisplayBackend {
    /// Create a new callback display backend
    pub fn new<F>(width: u32, height: u32, on_update: F) -> Self
    where
        F: Fn(&Framebuffer) + Send + Sync + 'static,
    {
        Self {
            width,
            height,
            connected: true,
            on_update: Box::new(on_update),
        }
    }

    /// Set connected state
    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
    }
}

impl std::fmt::Debug for CallbackDisplayBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackDisplayBackend")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("connected", &self.connected)
            .finish()
    }
}

impl DisplayBackend for CallbackDisplayBackend {
    fn name(&self) -> &str {
        "callback"
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn resolution(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn set_resolution(&mut self, width: u32, height: u32) -> Result<(), DisplayError> {
        self.width = width;
        self.height = height;
        Ok(())
    }

    fn update(&mut self, framebuffer: &Framebuffer) -> Result<(), DisplayError> {
        if !self.connected {
            return Err(DisplayError::NotConnected);
        }
        (self.on_update)(framebuffer);
        Ok(())
    }

    fn update_region(
        &mut self,
        framebuffer: &Framebuffer,
        _region: Rect,
    ) -> Result<(), DisplayError> {
        // Callback backend always updates full framebuffer
        self.update(framebuffer)
    }
}

/// Display statistics
#[derive(Debug, Clone, Default)]
pub struct DisplayStats {
    /// Total frames rendered
    pub frames: u64,
    /// Total bytes transferred
    pub bytes_transferred: u64,
    /// Number of partial updates
    pub partial_updates: u64,
    /// Number of full updates
    pub full_updates: u64,
}

impl DisplayStats {
    /// Reset statistics
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Display manager for coordinating multiple outputs
pub struct DisplayManager {
    backends: Vec<Box<dyn DisplayBackend>>,
    primary: usize,
    stats: DisplayStats,
}

impl std::fmt::Debug for DisplayManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DisplayManager")
            .field("backend_count", &self.backends.len())
            .field("primary", &self.primary)
            .field("stats", &self.stats)
            .finish()
    }
}

impl DisplayManager {
    /// Create a new display manager
    pub fn new() -> Self {
        Self {
            backends: Vec::new(),
            primary: 0,
            stats: DisplayStats::default(),
        }
    }

    /// Add a display backend
    pub fn add_backend(&mut self, backend: Box<dyn DisplayBackend>) -> usize {
        let index = self.backends.len();
        self.backends.push(backend);
        index
    }

    /// Remove a display backend
    pub fn remove_backend(&mut self, index: usize) -> Option<Box<dyn DisplayBackend>> {
        if index < self.backends.len() {
            Some(self.backends.remove(index))
        } else {
            None
        }
    }

    /// Get backend count
    pub fn backend_count(&self) -> usize {
        self.backends.len()
    }

    /// Get a backend
    pub fn get_backend(&self, index: usize) -> Option<&dyn DisplayBackend> {
        self.backends.get(index).map(|b| b.as_ref())
    }

    /// Get a backend mutably
    pub fn get_backend_mut(&mut self, index: usize) -> Option<&mut Box<dyn DisplayBackend>> {
        self.backends.get_mut(index)
    }

    /// Set primary display
    pub fn set_primary(&mut self, index: usize) {
        if index < self.backends.len() {
            self.primary = index;
        }
    }

    /// Get primary display index
    pub fn primary(&self) -> usize {
        self.primary
    }

    /// Update all displays
    pub fn update_all(&mut self, framebuffer: &Framebuffer) -> Vec<(usize, Result<(), DisplayError>)> {
        let mut results = Vec::new();
        for (i, backend) in self.backends.iter_mut().enumerate() {
            let result = backend.update(framebuffer);
            if result.is_ok() {
                self.stats.frames += 1;
                self.stats.bytes_transferred += framebuffer.buffer().len() as u64;
                self.stats.full_updates += 1;
            }
            results.push((i, result));
        }
        results
    }

    /// Update primary display
    pub fn update_primary(&mut self, framebuffer: &Framebuffer) -> Result<(), DisplayError> {
        if self.primary >= self.backends.len() {
            return Err(DisplayError::NotConnected);
        }
        let result = self.backends[self.primary].update(framebuffer);
        if result.is_ok() {
            self.stats.frames += 1;
            self.stats.bytes_transferred += framebuffer.buffer().len() as u64;
            self.stats.full_updates += 1;
        }
        result
    }

    /// Update a region on primary display
    pub fn update_region(
        &mut self,
        framebuffer: &Framebuffer,
        region: Rect,
    ) -> Result<(), DisplayError> {
        if self.primary >= self.backends.len() {
            return Err(DisplayError::NotConnected);
        }
        let result = self.backends[self.primary].update_region(framebuffer, region);
        if result.is_ok() {
            self.stats.frames += 1;
            self.stats.partial_updates += 1;
        }
        result
    }

    /// Get statistics
    pub fn stats(&self) -> &DisplayStats {
        &self.stats
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.stats.reset();
    }
}

impl Default for DisplayManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe display backend wrapper
pub type SharedDisplayBackend = Arc<RwLock<Box<dyn DisplayBackend>>>;

/// Create a shared display backend
pub fn shared_display_backend(backend: Box<dyn DisplayBackend>) -> SharedDisplayBackend {
    Arc::new(RwLock::new(backend))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_error() {
        let err = DisplayError::NotConnected;
        assert_eq!(format!("{}", err), "Display not connected");

        let err = DisplayError::BackendError("test".to_string());
        assert!(format!("{}", err).contains("test"));
    }

    #[test]
    fn test_null_backend() {
        let mut backend = NullDisplayBackend::new(1024, 768);
        assert_eq!(backend.name(), "null");
        assert!(backend.is_connected());
        assert_eq!(backend.resolution(), (1024, 768));

        backend.set_resolution(800, 600).unwrap();
        assert_eq!(backend.resolution(), (800, 600));
    }

    #[test]
    fn test_null_backend_update() {
        let mut backend = NullDisplayBackend::default();
        let fb = Framebuffer::with_dimensions(100, 100);

        backend.update(&fb).unwrap();
        backend.update_region(&fb, Rect::new(0, 0, 50, 50)).unwrap();
    }

    #[test]
    fn test_null_backend_disconnected() {
        let mut backend = NullDisplayBackend::default();
        backend.set_connected(false);

        let fb = Framebuffer::with_dimensions(100, 100);
        assert!(backend.update(&fb).is_err());
    }

    #[test]
    fn test_memory_backend() {
        let mut backend = MemoryDisplayBackend::new(100, 100);
        assert_eq!(backend.name(), "memory");
        assert!(backend.is_connected());
        assert_eq!(backend.resolution(), (100, 100));
        assert_eq!(backend.update_count(), 0);
    }

    #[test]
    fn test_memory_backend_update() {
        let mut backend = MemoryDisplayBackend::new(100, 100);
        let mut fb = Framebuffer::with_dimensions(100, 100);

        fb.set_pixel(10, 10, Color::RED);
        backend.update(&fb).unwrap();

        assert_eq!(backend.update_count(), 1);
        let pixel = backend.get_pixel(10, 10).unwrap();
        assert_eq!(pixel.r, 255);
    }

    #[test]
    fn test_memory_backend_region_update() {
        let mut backend = MemoryDisplayBackend::new(100, 100);
        let mut fb = Framebuffer::with_dimensions(100, 100);

        fb.set_pixel(25, 25, Color::GREEN);
        backend
            .update_region(&fb, Rect::new(20, 20, 10, 10))
            .unwrap();

        assert_eq!(backend.update_count(), 1);
        let pixel = backend.get_pixel(25, 25).unwrap();
        assert_eq!(pixel.g, 255);
    }

    #[test]
    fn test_memory_backend_resize() {
        let mut backend = MemoryDisplayBackend::default();
        backend.set_resolution(1920, 1080).unwrap();
        assert_eq!(backend.resolution(), (1920, 1080));
    }

    #[test]
    fn test_memory_backend_title() {
        let mut backend = MemoryDisplayBackend::default();
        backend.set_title("Test Window");
        assert_eq!(backend.title(), "Test Window");
    }

    #[test]
    fn test_memory_backend_cursor() {
        let mut backend = MemoryDisplayBackend::default();

        backend.show_cursor(false);
        backend.set_cursor_position(100, 200);

        let (visible, x, y) = backend.cursor_state();
        assert!(!visible);
        assert_eq!(x, 100);
        assert_eq!(y, 200);
    }

    #[test]
    fn test_callback_backend() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let mut backend = CallbackDisplayBackend::new(800, 600, move |_fb| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        let fb = Framebuffer::with_dimensions(800, 600);
        backend.update(&fb).unwrap();
        backend.update(&fb).unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_display_manager() {
        let mut manager = DisplayManager::new();

        let null_backend = Box::new(NullDisplayBackend::default());
        let memory_backend = Box::new(MemoryDisplayBackend::default());

        let idx1 = manager.add_backend(null_backend);
        let idx2 = manager.add_backend(memory_backend);

        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        assert_eq!(manager.backend_count(), 2);
    }

    #[test]
    fn test_display_manager_primary() {
        let mut manager = DisplayManager::new();
        manager.add_backend(Box::new(NullDisplayBackend::default()));
        manager.add_backend(Box::new(MemoryDisplayBackend::default()));

        assert_eq!(manager.primary(), 0);
        manager.set_primary(1);
        assert_eq!(manager.primary(), 1);
    }

    #[test]
    fn test_display_manager_update_all() {
        let mut manager = DisplayManager::new();
        manager.add_backend(Box::new(NullDisplayBackend::default()));
        manager.add_backend(Box::new(MemoryDisplayBackend::default()));

        let fb = Framebuffer::with_dimensions(100, 100);
        let results = manager.update_all(&fb);

        assert_eq!(results.len(), 2);
        assert!(results[0].1.is_ok());
        assert!(results[1].1.is_ok());
    }

    #[test]
    fn test_display_manager_update_primary() {
        let mut manager = DisplayManager::new();
        manager.add_backend(Box::new(MemoryDisplayBackend::new(100, 100)));

        let mut fb = Framebuffer::with_dimensions(100, 100);
        fb.set_pixel(50, 50, Color::BLUE);

        manager.update_primary(&fb).unwrap();

        assert_eq!(manager.stats().frames, 1);
        assert_eq!(manager.stats().full_updates, 1);
    }

    #[test]
    fn test_display_manager_stats() {
        let mut manager = DisplayManager::new();
        manager.add_backend(Box::new(NullDisplayBackend::default()));

        let fb = Framebuffer::with_dimensions(100, 100);

        manager.update_primary(&fb).unwrap();
        manager.update_primary(&fb).unwrap();
        manager.update_region(&fb, Rect::new(0, 0, 50, 50)).unwrap();

        let stats = manager.stats();
        assert_eq!(stats.frames, 3);
        assert_eq!(stats.full_updates, 2);
        assert_eq!(stats.partial_updates, 1);

        manager.reset_stats();
        assert_eq!(manager.stats().frames, 0);
    }

    #[test]
    fn test_display_manager_remove_backend() {
        let mut manager = DisplayManager::new();
        manager.add_backend(Box::new(NullDisplayBackend::default()));
        manager.add_backend(Box::new(MemoryDisplayBackend::default()));

        let removed = manager.remove_backend(0);
        assert!(removed.is_some());
        assert_eq!(manager.backend_count(), 1);
    }

    #[test]
    fn test_display_manager_get_backend() {
        let mut manager = DisplayManager::new();
        manager.add_backend(Box::new(NullDisplayBackend::new(1920, 1080)));

        let backend = manager.get_backend(0).unwrap();
        assert_eq!(backend.name(), "null");
        assert_eq!(backend.resolution(), (1920, 1080));
    }

    #[test]
    fn test_display_manager_no_primary() {
        let mut manager = DisplayManager::new();
        let fb = Framebuffer::with_dimensions(100, 100);

        let result = manager.update_primary(&fb);
        assert!(matches!(result, Err(DisplayError::NotConnected)));
    }

    #[test]
    fn test_shared_display_backend() {
        let backend = shared_display_backend(Box::new(MemoryDisplayBackend::default()));

        {
            let mut guard = backend.write().unwrap();
            guard.set_resolution(1280, 720).unwrap();
        }

        {
            let guard = backend.read().unwrap();
            assert_eq!(guard.resolution(), (1280, 720));
        }
    }
}
