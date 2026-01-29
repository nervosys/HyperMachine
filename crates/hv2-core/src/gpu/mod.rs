//! GPU Subsystem
//!
//! This module provides GPU and display emulation including:
//! - Core GPU types (pixel formats, colors, display modes)
//! - Software framebuffer with pixel operations
//! - VirtIO-GPU device with 2D operations

pub mod core;
pub mod framebuffer;
pub mod virtio_gpu;

// Re-export key types
pub use core::{
    Color, CursorShape, CursorState, DisplayMode, DisplaySurface, GpuStats, PixelFormat, Rect,
    Scanout,
};

pub use framebuffer::{DoubleBuffer, Framebuffer};

pub use virtio_gpu::{
    DisplayInfo, GpuResource, ScanoutState, VirtioGpu, VirtioGpuCtrlType, VirtioGpuError,
    VirtioGpuFormat, VirtioGpuStats,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Verify key types are exported
        let _ = PixelFormat::Argb32;
        let _ = Color::BLACK;
        let _ = DisplayMode::VGA;
    }

    #[test]
    fn test_create_framebuffer() {
        let fb = Framebuffer::new(DisplayMode::VGA);
        assert_eq!(fb.width(), 640);
        assert_eq!(fb.height(), 480);
    }

    #[test]
    fn test_create_virtio_gpu() {
        let gpu = VirtioGpu::new("gpu0", 1024, 768);
        assert_eq!(gpu.name(), "gpu0");
    }

    #[test]
    fn test_color_operations() {
        let color = Color::rgb(255, 128, 64);
        let argb = color.to_argb32();
        let restored = Color::from_argb32(argb);
        assert_eq!(color.r, restored.r);
        assert_eq!(color.g, restored.g);
        assert_eq!(color.b, restored.b);
    }

    #[test]
    fn test_rect_operations() {
        let r1 = Rect::new(0, 0, 100, 100);
        let r2 = Rect::new(50, 50, 100, 100);
        assert!(r1.intersects(&r2));

        let intersection = r1.intersection(&r2).unwrap();
        assert_eq!(intersection.x, 50);
        assert_eq!(intersection.y, 50);
        assert_eq!(intersection.width, 50);
        assert_eq!(intersection.height, 50);
    }

    #[test]
    fn test_double_buffer() {
        let mut db = DoubleBuffer::new(DisplayMode::new(100, 100, PixelFormat::Xrgb32));
        db.back_buffer().set_pixel(50, 50, Color::RED);
        db.swap();
        assert_eq!(db.front_buffer().get_pixel(50, 50).r, 255);
    }
}
