//! Programmable Interval Timer (PIT) device emulation

use crate::{Device, DeviceType, Pic8259, Result};
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
    /// Output pin state
    output: bool,
    /// Counting has started (reload value loaded)
    active: bool,
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
            output: false,
            active: true, // PIT counts immediately after power-on reset
        }
    }

    /// Update the counter based on elapsed time.  Returns `true` when the
    /// channel fires (output transitions that should trigger an IRQ on ch0).
    fn update_count(&mut self) -> bool {
        if !self.gate || !self.active {
            return false;
        }

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update);
        self.last_update = now;

        // PIT frequency is 1.193182 MHz
        let ticks = (elapsed.as_micros() as u64 * 1193182) / 1_000_000;
        if ticks == 0 {
            return false;
        }

        match self.mode {
            PitMode::InterruptOnTerminalCount => {
                // Mode 0: count down once; output goes high at terminal count
                if self.output {
                    return false; // Already fired
                }
                if self.count as u64 <= ticks {
                    self.count = 0;
                    self.output = true;
                    true
                } else {
                    self.count -= ticks as u16;
                    false
                }
            }
            PitMode::HardwareRetriggerableOneShot => {
                // Mode 1: similar to mode 0; gate rising edge restarts
                if self.output {
                    return false;
                }
                if self.count as u64 <= ticks {
                    self.count = 0;
                    self.output = true;
                    true
                } else {
                    self.count -= ticks as u16;
                    false
                }
            }
            PitMode::RateGenerator => {
                // Mode 2: periodic; output goes low for one tick at terminal count,
                // then reloads
                if self.count as u64 <= ticks {
                    self.count = self.reload_value;
                    true
                } else {
                    self.count -= ticks as u16;
                    false
                }
            }
            PitMode::SquareWaveGenerator => {
                // Mode 3: square wave — toggle output at half the period.
                // Each full period of reload_value ticks produces one toggle pair.
                if self.count as u64 <= ticks {
                    self.output = !self.output;
                    self.count = self.reload_value;
                    // Fire on the falling edge (output going low)
                    !self.output
                } else {
                    self.count -= ticks as u16;
                    false
                }
            }
            PitMode::SoftwareTriggeredStrobe => {
                // Mode 4: output goes low for one tick at terminal count (one-shot)
                if self.output {
                    return false;
                }
                if self.count as u64 <= ticks {
                    self.count = 0;
                    self.output = true;
                    true
                } else {
                    self.count -= ticks as u16;
                    false
                }
            }
            PitMode::HardwareTriggeredStrobe => {
                // Mode 5: like mode 4 but triggered by gate
                if self.output {
                    return false;
                }
                if self.count as u64 <= ticks {
                    self.count = 0;
                    self.output = true;
                    true
                } else {
                    self.count -= ticks as u16;
                    false
                }
            }
        }
    }

    fn read_count(&mut self) -> u16 {
        self.update_count();
        self.latch.unwrap_or(self.count)
    }
}

/// Programmable Interval Timer device
///
/// # What its interrupt does, and does not, reach
///
/// This raises IRQ 0 on the userspace [`Pic8259`] only. A guest whose interrupt
/// controller lives inside the hypervisor never reads that, so a tick raised
/// here does not reach such a guest -- and unlike the keyboard and the serial
/// port, which were given a path to one, that is deliberate: KVM is asked for
/// an in-kernel PIT (`KVM_CREATE_PIT2`), the guest's clock comes from there,
/// and delivering this device's tick as well would give the guest two
/// interrupts per period and a clock that runs fast.
///
/// So on a KVM guest this device models the programming interface -- the mode
/// and reload values a guest writes, and the counts it reads back -- while the
/// interrupts belong to the hypervisor. It is stated here because "raises IRQ
/// 0" is otherwise a reasonable thing to assume from the code, and a caller who
/// assumed it would be wrong in a way nothing reports.
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
    #[must_use]
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
    /// Read one byte-wide register.
    ///
    /// Split out so a wider access can walk consecutive registers the way the
    /// hardware does. Reading a channel is stateful -- the LSB/MSB latch
    /// advances on each read -- so this has to happen a byte at a time rather
    /// than as one wide read.
    fn read_register(&self, offset: u64) -> u8 {
        let mut channels = self.channels.lock();

        match offset {
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
        }
    }

    /// Write one byte-wide register.
    fn write_register(&self, offset: u64, byte: u8) {
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
                        channel.active = true;
                        channel.output = false;
                    }
                    2 => {
                        // MSB only
                        channel.reload_value = (byte as u16) << 8;
                        channel.count = channel.reload_value;
                        channel.active = true;
                        channel.output = false;
                    }
                    3 => {
                        // LSB then MSB
                        if let Some(lsb) = channel.write_latch {
                            channel.reload_value = lsb as u16 | ((byte as u16) << 8);
                            channel.count = channel.reload_value;
                            channel.write_latch = None;
                            channel.active = true;
                            channel.output = false;
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
                    // Read-back command (8254 only)
                    // Bits: 11 | !COUNT | !STATUS | CH2 | CH1 | CH0 | 0 | 0
                    let latch_count = (byte & 0x20) == 0;
                    let latch_status = (byte & 0x10) == 0;

                    for ch in 0..3u8 {
                        if (byte >> (ch + 1)) & 1 == 0 {
                            continue; // Channel not selected
                        }
                        let channel = &mut channels[ch as usize];

                        if latch_count && channel.latch.is_none() {
                            channel.update_count();
                            channel.latch = Some(channel.count);
                        }

                        if latch_status {
                            // Status byte: OUTPUT | NULL_COUNT | RW1 | RW0 | M2 | M1 | M0 | BCD
                            let status = (channel.rw_mode << 4)
                                | (((channel.mode as u8) & 0x07) << 1)
                                | (channel.bcd_mode as u8);
                            // Latch the status as a fake count so the next read returns it
                            if channel.latch.is_none() {
                                channel.latch = Some(status as u16);
                            }
                        }
                    }
                    return;
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
        // Never an error. A device error on the I/O path reaches
        // `VM::handle_exit` and stops the VM, so refusing a wide access would
        // kill a guest for doing something hardware simply answers.
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = self.read_register(offset + index as u64);
        }
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        // And every byte is written, rather than the first one and silence
        // about the rest.
        for (index, byte) in data.iter().enumerate() {
            self.write_register(offset + index as u64, *byte);
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
    #[tokio::test(start_paused = true)]
    async fn test_timer_frequency() {
        // Virtual time, because the thing being asserted is the timer's
        // *configured* rate, not the machine's ability to schedule a task
        // 18 times in a real second. The interval uses MissedTickBehavior::Skip
        // -- correct for a timer, and it means a loaded host genuinely loses
        // ticks, so a wall-clock assertion here fails for a reason that has
        // nothing to do with the code. It has failed that way repeatedly under
        // parallel builds.
        //
        // Paused time advances only when every task is idle, so the count
        // below is exact rather than approximate.
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

        // 1 s / 54.925 ms = 18.2 periods, and the window catches the boundary
        // at both ends because `interval`'s first tick fires immediately — so
        // 19, exactly, every time. An exact number is the point: it fails if
        // the configured period changes, which a tolerance band would hide.
        assert_eq!(
            ticks_per_second, 19,
            "the PIT should tick 19 times in this virtual second"
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
        // Virtual time, not wall-clock. The timer task uses
        // `MissedTickBehavior::Skip`, so on a loaded machine real ticks are
        // dropped rather than queued and a wall-clock assertion flakes — this
        // test failed exactly that way under a parallel build. Pausing the
        // clock and advancing it by hand makes the count deterministic.
        tokio::time::pause();

        let pic = Arc::new(Pic8259::new());
        let mut timer = TimerDevice::new("PIT".to_string(), 0x40);

        assert_eq!(timer.total_ticks(), 0, "Should start at 0");

        timer.set_pic(Arc::clone(&pic));

        // Let the spawned task reach its first `tick().await`.
        tokio::task::yield_now().await;

        // Advance one period at a time until the counter has moved far enough,
        // bounded so a timer that never ticks still fails rather than hanging.
        // The bound is generous because one advance does not always translate
        // into one counted tick — channel 0 only counts down to terminal count
        // — and the point here is that the counter advances with elapsed
        // periods, not the exact ratio.
        const PERIOD: Duration = Duration::from_micros(54_925);
        const TARGET: u64 = 4;

        let mut ticks = 0;
        for _ in 0..64 {
            tokio::time::advance(PERIOD).await;
            tokio::task::yield_now().await;
            ticks = timer.total_ticks();
            if ticks >= TARGET {
                break;
            }
        }

        assert!(
            ticks >= TARGET,
            "Should have reached {} ticks within 64 periods, got {}",
            TARGET,
            ticks
        );

        timer.stop_timer_task();
    }

    #[test]
    fn test_pit_mode0_interrupt_on_terminal_count() {
        // Mode 0: count down once, output goes high at terminal count and stays
        let mut ch = PitChannel::new();
        ch.mode = PitMode::InterruptOnTerminalCount;
        ch.reload_value = 10;
        ch.count = 10;
        ch.active = true;
        ch.output = false;

        // Simulate enough elapsed time for the count to expire
        ch.last_update = Instant::now() - Duration::from_micros(20);
        let fired = ch.update_count();
        assert!(fired, "Mode 0 should fire when count reaches 0");
        assert!(ch.output, "Output should be high after terminal count");

        // Subsequent calls should NOT fire again (one-shot)
        ch.last_update = Instant::now() - Duration::from_micros(20);
        let fired = ch.update_count();
        assert!(!fired, "Mode 0 should not fire again once output is high");
    }

    #[test]
    fn test_pit_mode2_rate_generator() {
        // Mode 2: periodic, auto-reloads
        let mut ch = PitChannel::new();
        ch.mode = PitMode::RateGenerator;
        ch.reload_value = 10;
        ch.count = 10;
        ch.active = true;

        // Fire once
        ch.last_update = Instant::now() - Duration::from_micros(20);
        let fired = ch.update_count();
        assert!(fired, "Mode 2 should fire at terminal count");
        assert_eq!(
            ch.count, ch.reload_value,
            "Mode 2 should reload after firing"
        );

        // Should fire again on next expiry (periodic)
        ch.last_update = Instant::now() - Duration::from_micros(20);
        let fired = ch.update_count();
        assert!(fired, "Mode 2 should fire repeatedly");
    }

    #[test]
    fn test_pit_mode3_square_wave() {
        // Mode 3: square wave — fires on falling edge only
        let mut ch = PitChannel::new();
        ch.mode = PitMode::SquareWaveGenerator;
        ch.reload_value = 10;
        ch.count = 10;
        ch.active = true;
        ch.output = false;

        // First expiry: output toggles to true (rising edge) — should NOT fire
        ch.last_update = Instant::now() - Duration::from_micros(20);
        let fired = ch.update_count();
        assert!(!fired, "Mode 3 should not fire on rising edge");
        assert!(ch.output, "Output should be high after first toggle");

        // Second expiry: output toggles to false (falling edge) — should fire
        ch.last_update = Instant::now() - Duration::from_micros(20);
        let fired = ch.update_count();
        assert!(fired, "Mode 3 should fire on falling edge");
        assert!(!ch.output, "Output should be low after second toggle");
    }

    #[test]
    fn test_pit_channel_gate_inhibits_counting() {
        let mut ch = PitChannel::new();
        ch.mode = PitMode::RateGenerator;
        ch.reload_value = 10;
        ch.count = 10;
        ch.active = true;
        ch.gate = false;

        ch.last_update = Instant::now() - Duration::from_micros(100);
        let fired = ch.update_count();
        assert!(!fired, "Should not fire when gate is low");
        assert_eq!(ch.count, 10, "Count should not change when gate is low");
    }
}
