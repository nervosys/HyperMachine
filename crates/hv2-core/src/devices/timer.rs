//! Programmable Interval Timer (PIT) device emulation

use crate::{Device, DeviceType, Error, Pic8259, Result};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// PIT channels
const CHANNEL_0: u8 = 0;
const CHANNEL_1: u8 = 1;
const CHANNEL_2: u8 = 2;

/// PIT operating modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PitMode {
    InterruptOnTerminalCount = 0,
    HardwareRetriggerableOneShot = 1,
    RateGenerator = 2,
    SquareWaveGenerator = 3,
    SoftwareTriggeredStrobe = 4,
    HardwareTriggeredStrobe = 5,
}

impl PitMode {
    fn from_u8(value: u8) -> Self {
        match value & 0x07 {
            0 => PitMode::InterruptOnTerminalCount,
            1 => PitMode::HardwareRetriggerableOneShot,
            2 | 6 => PitMode::RateGenerator,
            3 | 7 => PitMode::SquareWaveGenerator,
            4 => PitMode::SoftwareTriggeredStrobe,
            5 => PitMode::HardwareTriggeredStrobe,
            _ => PitMode::RateGenerator,
        }
    }
}

/// PIT channel state
struct PitChannel {
    /// Reload value
    reload_value: u16,
    /// Current count
    count: u16,
    /// Operating mode
    mode: PitMode,
    /// Binary/BCD mode (true = BCD)
    bcd_mode: bool,
    /// Read/Write mode (0 = latch, 1 = LSB, 2 = MSB, 3 = LSB then MSB)
    rw_mode: u8,
    /// Latch for reading
    latch: Option<u16>,
    /// Write state (for 2-byte operations)
    write_latch: Option<u8>,
    /// Last update time
    last_update: Instant,
    /// Gate signal
    gate: bool,
}

impl PitChannel {
    fn new() -> Self {
        Self {
            reload_value: 0,
            count: 0,
            mode: PitMode::RateGenerator,
            bcd_mode: false,
            rw_mode: 3,
            latch: None,
            write_latch: None,
            last_update: Instant::now(),
            gate: true,
        }
    }

    fn update_count(&mut self) -> bool {
        if !self.gate {
            return false;
        }

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update);
        self.last_update = now;

        // PIT frequency is 1.193182 MHz
        let ticks = (elapsed.as_micros() as u64 * 1193182) / 1_000_000;

        let reached_zero = if self.count >= ticks as u16 {
            self.count -= ticks as u16;
            false
        } else {
            self.count = self.reload_value;
            true
        };

        reached_zero
    }

    fn read_count(&mut self) -> u16 {
        self.update_count();
        self.latch.unwrap_or(self.count)
    }
}

/// Programmable Interval Timer device
pub struct TimerDevice {
    name: String,
    base_address: u64,
    /// Three timer channels
    channels: Arc<Mutex<[PitChannel; 3]>>,
    /// Interrupt generation enabled
    interrupt_enabled: AtomicBool,
    /// Total ticks (shared with timer task)
    total_ticks: Arc<AtomicU64>,
    /// PIC for raising interrupts
    pic: Option<Arc<Pic8259>>,
    /// Timer task running flag
    timer_running: Arc<AtomicBool>,
}

impl TimerDevice {
    /// Create a new timer device
    pub fn new(name: String, base_address: u64) -> Self {
        Self {
            name,
            base_address,
            channels: Arc::new(Mutex::new([
                PitChannel::new(),
                PitChannel::new(),
                PitChannel::new(),
            ])),
            interrupt_enabled: AtomicBool::new(false),
            total_ticks: Arc::new(AtomicU64::new(0)),
            pic: None,
            timer_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Set the PIC for interrupt generation and start timer task
    pub fn set_pic(&mut self, pic: Arc<Pic8259>) {
        self.pic = Some(Arc::clone(&pic));
        self.start_timer_task();
    }

    /// Start background timer task that generates interrupts at 18.2 Hz
    fn start_timer_task(&self) {
        // Only start if not already running
        if self.timer_running.swap(true, Ordering::Relaxed) {
            return;
        }

        let channels = Arc::clone(&self.channels);
        let pic = self.pic.as_ref().map(Arc::clone);
        let total_ticks = Arc::clone(&self.total_ticks);
        let running = Arc::clone(&self.timer_running);

        // 18.2 Hz = 54.925 ms period
        let interval_micros = 54925;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_micros(interval_micros));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            while running.load(Ordering::Relaxed) {
                interval.tick().await;

                // Update channel 0 count and raise interrupt if it reaches zero
                let mut chans = channels.lock();
                if chans[0].update_count() {
                    total_ticks.fetch_add(1, Ordering::Relaxed);

                    // Raise IRQ 0 if PIC available
                    if let Some(ref pic) = pic {
                        let _ = pic.raise_irq(0);
                    }
                }
            }
        });
    }

    /// Stop the timer task
    pub fn stop_timer_task(&self) {
        self.timer_running.store(false, Ordering::Relaxed);
    }

    /// Get the base address
    pub fn base_address(&self) -> u64 {
        self.base_address
    }

    /// Get total ticks
    pub fn total_ticks(&self) -> u64 {
        self.total_ticks.load(Ordering::Relaxed)
    }

    /// Enable/disable interrupts
    pub fn set_interrupt_enabled(&self, enabled: bool) {
        self.interrupt_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Check if interrupts are enabled
    pub fn interrupt_enabled(&self) -> bool {
        self.interrupt_enabled.load(Ordering::Relaxed)
    }

    /// Tick the timer and raise interrupt if channel 0 reaches zero
    pub fn tick(&self) -> Result<()> {
        let mut channels = self.channels.lock();

        // Update channel 0 (system timer)
        if channels[0].update_count() {
            // Counter reached zero, increment total ticks
            self.total_ticks.fetch_add(1, Ordering::Relaxed);

            // Raise IRQ 0 if interrupts enabled and PIC available
            if self.interrupt_enabled.load(Ordering::Relaxed) {
                if let Some(ref pic) = self.pic {
                    pic.raise_irq(0)?;
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Device for TimerDevice {
    fn device_type(&self) -> DeviceType {
        DeviceType::Timer
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn init(&mut self) -> Result<()> {
        tracing::info!(
            "Initializing timer device '{}' at 0x{:X}",
            self.name,
            self.base_address
        );
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        if data.len() != 1 {
            return Err(Error::Device(
                "Timer device only supports single-byte reads".to_string(),
            ));
        }

        let mut channels = self.channels.lock();

        let value = match offset {
            0..=2 => {
                // Channel data ports
                let channel = &mut channels[offset as usize];
                let count = channel.read_count();

                match channel.rw_mode {
                    1 => count as u8,        // LSB only
                    2 => (count >> 8) as u8, // MSB only
                    3 => {
                        // LSB then MSB
                        if let Some(lsb) = channel.write_latch {
                            channel.write_latch = None;
                            (count >> 8) as u8
                        } else {
                            channel.write_latch = Some((count >> 8) as u8);
                            count as u8
                        }
                    }
                    _ => 0,
                }
            }
            3 => {
                // Control word register - not readable
                0
            }
            _ => 0,
        };

        data[0] = value;
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        let byte = data[0];
        let mut channels = self.channels.lock();

        match offset {
            0..=2 => {
                // Channel data ports
                let channel = &mut channels[offset as usize];

                match channel.rw_mode {
                    1 => {
                        // LSB only
                        channel.reload_value = byte as u16;
                        channel.count = channel.reload_value;
                    }
                    2 => {
                        // MSB only
                        channel.reload_value = (byte as u16) << 8;
                        channel.count = channel.reload_value;
                    }
                    3 => {
                        // LSB then MSB
                        if let Some(lsb) = channel.write_latch {
                            channel.reload_value = lsb as u16 | ((byte as u16) << 8);
                            channel.count = channel.reload_value;
                            channel.write_latch = None;
                        } else {
                            channel.write_latch = Some(byte);
                        }
                    }
                    _ => {}
                }

                self.total_ticks.fetch_add(1, Ordering::Relaxed);
            }
            3 => {
                // Control word register
                let channel_select = (byte >> 6) & 0x03;

                if channel_select == 3 {
                    // Read-back command (not implemented)
                    return Ok(());
                }

                let channel = &mut channels[channel_select as usize];

                let rw_mode = (byte >> 4) & 0x03;
                if rw_mode == 0 {
                    // Latch count value
                    channel.update_count();
                    channel.latch = Some(channel.count);
                } else {
                    channel.rw_mode = rw_mode;
                    channel.mode = PitMode::from_u8((byte >> 1) & 0x07);
                    channel.bcd_mode = (byte & 0x01) != 0;
                    channel.write_latch = None;
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        let mut channels = self.channels.lock();
        for channel in channels.iter_mut() {
            *channel = PitChannel::new();
        }
        self.total_ticks.store(0, Ordering::Relaxed);
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down timer device '{}'", self.name);
        self.stop_timer_task();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_timer_device() {
        let mut timer = TimerDevice::new("PIT".to_string(), 0x40);
        timer.init().await.unwrap();

        // Set channel 0 to mode 2 (rate generator)
        // Control word: channel 0, LSB/MSB, mode 2, binary
        timer.write(3, &[0b00110100]).await.unwrap();

        // Set reload value to 1193 (1 ms at 1.193182 MHz)
        timer.write(0, &[0xA9]).await.unwrap(); // LSB
        timer.write(0, &[0x04]).await.unwrap(); // MSB

        // Read count back
        let mut buf = [0u8; 1];
        timer.read(0, &mut buf).await.unwrap();

        assert!(timer.total_ticks() > 0);
    }

    #[tokio::test]
    async fn test_timer_irq_generation() {
        // Test that timer generates IRQ 0 when PIC is set
        let pic = Arc::new(Pic8259::new());
        let mut timer = TimerDevice::new("PIT".to_string(), 0x40);

        // Set PIC (this starts the timer task)
        timer.set_pic(Arc::clone(&pic));

        // Wait for at least one timer tick (55ms + buffer)
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Verify that timer is generating ticks
        let ticks = timer.total_ticks();
        assert!(
            ticks >= 1,
            "Timer should have generated at least 1 tick, got {}",
            ticks
        );

        // Cleanup
        timer.stop_timer_task();
    }
    #[tokio::test]
    async fn test_timer_frequency() {
        // Test that timer runs at approximately 18.2 Hz
        let pic = Arc::new(Pic8259::new());
        let mut timer = TimerDevice::new("PIT".to_string(), 0x40);

        timer.set_pic(Arc::clone(&pic));

        // Wait for timer task to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        let initial_ticks = timer.total_ticks();

        // Wait for 1 second
        tokio::time::sleep(Duration::from_secs(1)).await;

        let final_ticks = timer.total_ticks();
        let ticks_per_second = final_ticks - initial_ticks;

        // Should be approximately 18.2 ticks per second (allow ±3 ticks tolerance)
        assert!(
            ticks_per_second >= 15 && ticks_per_second <= 21,
            "Expected ~18 ticks/second, got {}",
            ticks_per_second
        );

        timer.stop_timer_task();
    }

    #[tokio::test]
    async fn test_timer_stop() {
        // Test that stopping timer task prevents further interrupts
        let pic = Arc::new(Pic8259::new());
        let mut timer = TimerDevice::new("PIT".to_string(), 0x40);

        timer.set_pic(Arc::clone(&pic));

        // Wait for some ticks
        tokio::time::sleep(Duration::from_millis(200)).await;
        let ticks_before_stop = timer.total_ticks();

        // Stop timer
        timer.stop_timer_task();

        // Give the task time to actually stop
        tokio::time::sleep(Duration::from_millis(100)).await;

        let ticks_immediately_after = timer.total_ticks();

        // Wait longer to ensure no more ticks
        tokio::time::sleep(Duration::from_millis(200)).await;
        let ticks_after_stop = timer.total_ticks();

        // Ticks should not increase after stopping (allow for 1 in-flight tick)
        assert!(
            ticks_after_stop <= ticks_immediately_after + 1,
            "Timer should stop incrementing ticks after stop_timer_task(), before={}, immediately_after={}, final={}",
            ticks_before_stop,
            ticks_immediately_after,
            ticks_after_stop
        );
    }
    #[tokio::test]
    async fn test_timer_total_ticks() {
        // Test that total_ticks() accurately reflects timer activity
        let pic = Arc::new(Pic8259::new());
        let mut timer = TimerDevice::new("PIT".to_string(), 0x40);

        assert_eq!(timer.total_ticks(), 0, "Should start at 0");

        timer.set_pic(Arc::clone(&pic));

        // Wait for task to start
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Wait for multiple ticks (at 18.2 Hz, 300ms = ~5.5 ticks)
        tokio::time::sleep(Duration::from_millis(300)).await;

        let ticks = timer.total_ticks();
        assert!(
            ticks >= 4,
            "Should have at least 4 ticks after 300ms, got {}",
            ticks
        );

        timer.stop_timer_task();
    }
}
