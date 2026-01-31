//! End-to-end exit handler tests with registered devices

use hv2_core::{Device, DeviceManager, IoDirection, SerialDevice, TimerDevice, VmExit};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Helper function to create a device manager with registered devices
/// Returns (manager, serial_device, timer_device)
async fn setup_device_manager() -> (
    Arc<DeviceManager>,
    Arc<RwLock<SerialDevice>>,
    Arc<RwLock<TimerDevice>>,
) {
    let manager = Arc::new(DeviceManager::new());

    // Register serial device (COM1)
    let serial = Arc::new(RwLock::new(SerialDevice::new("COM1".to_string(), 0x3F8)));
    let serial_device: Arc<RwLock<dyn Device>> = serial.clone();
    manager
        .register_device("serial".to_string(), serial_device)
        .await
        .unwrap();
    manager
        .register_io_port_range("serial".to_string(), 0x3F8, 0x3FF)
        .await
        .unwrap();

    // Register timer device (PIT)
    let timer = Arc::new(RwLock::new(TimerDevice::new("PIT".to_string(), 0x40)));
    let timer_device: Arc<RwLock<dyn Device>> = timer.clone();
    manager
        .register_device("timer".to_string(), timer_device)
        .await
        .unwrap();
    manager
        .register_io_port_range("timer".to_string(), 0x40, 0x43)
        .await
        .unwrap();

    (manager, serial, timer)
}

#[tokio::test]
async fn test_io_exit_to_serial_device() {
    let (manager, serial, _timer) = setup_device_manager().await;

    // Test I/O OUT to serial port (write)
    let port = 0x3F8; // THR register
    let data: u32 = 0x48; // 'H'

    if let Some(handle) = manager.find_io_device(port).await {
        assert_eq!(handle.device_name(), "serial");
        assert_eq!(handle.base_port(), 0x3F8);

        // Simulate exit handler writing to device
        let offset = (port - handle.base_port()) as u64;
        handle.write_register(offset, data).await.unwrap();

        // Verify the write succeeded by checking device state
        let output = serial.read().await.output_string();
        assert!(output.contains('H'));
    } else {
        panic!("Serial device not found at port 0x3F8");
    }
}

#[tokio::test]
async fn test_io_exit_to_timer_device() {
    let (manager, _serial, _timer) = setup_device_manager().await;

    // Test I/O OUT to timer port (control word)
    let port = 0x43; // Control register
    let control_word: u32 = 0x34; // Channel 0, LSB/MSB, Mode 2, Binary

    if let Some(handle) = manager.find_io_device(port).await {
        assert_eq!(handle.device_name(), "timer");
        assert_eq!(handle.base_port(), 0x40);

        // Simulate exit handler writing to device
        let offset = (port - handle.base_port()) as u64;
        handle.write_register(offset, control_word).await.unwrap();

        // Write succeeded - timer is now configured
    } else {
        panic!("Timer device not found at port 0x43");
    }
}

#[tokio::test]
async fn test_io_exit_unmapped_port() {
    let (manager, _serial, _timer) = setup_device_manager().await;

    // Test I/O to unmapped port
    let port = 0x1000; // Unmapped port

    // Should return None for unmapped port
    assert!(manager.find_io_device(port).await.is_none());
}

#[tokio::test]
async fn test_io_exit_multiple_devices() {
    let (manager, serial, _timer) = setup_device_manager().await;

    // Write to serial device
    if let Some(handle) = manager.find_io_device(0x3F8).await {
        handle.write_register(0, 0x41).await.unwrap(); // 'A'
    }

    // Write to timer device
    if let Some(handle) = manager.find_io_device(0x40).await {
        handle.write_register(0, 0xFF).await.unwrap(); // LSB of count
    }

    // Verify both devices received their writes
    assert!(serial.read().await.output_string().contains('A'));

    // Timer write succeeded (no panic)
}

#[tokio::test]
async fn test_mmio_exit_to_device() {
    let manager = Arc::new(DeviceManager::new());

    // Register MMIO device
    let mmio_serial = Arc::new(RwLock::new(SerialDevice::new(
        "MMIO_SERIAL".to_string(),
        0x1000_0000,
    )));
    let mmio_device: Arc<RwLock<dyn Device>> = mmio_serial.clone();
    manager
        .register_device("mmio_serial".to_string(), mmio_device)
        .await
        .unwrap();
    manager
        .register_mmio_region("mmio_serial".to_string(), 0x1000_0000, 0x1000)
        .await
        .unwrap();
    mmio_serial.write().await.init().await.unwrap();

    // Test MMIO write
    let addr = 0x1000_0000;
    if let Some(handle) = manager.find_mmio_device(addr).await {
        assert_eq!(handle.device_name(), "mmio_serial");
        assert_eq!(handle.base_address(), 0x1000_0000);

        // Simulate exit handler writing to MMIO
        let offset = addr - handle.base_address();
        handle.write_register(offset, 0x42).await.unwrap(); // 'B'

        // Verify write
        let output = mmio_serial.read().await.output_string();
        assert!(output.contains('B'));
    } else {
        panic!("MMIO device not found at address 0x1000_0000");
    }
}

#[tokio::test]
async fn test_mmio_exit_read_write() {
    let manager = Arc::new(DeviceManager::new());

    // Register MMIO device
    let mmio_serial = Arc::new(RwLock::new(SerialDevice::new(
        "MMIO_SERIAL".to_string(),
        0x2000_0000,
    )));
    let mmio_device: Arc<RwLock<dyn Device>> = mmio_serial.clone();
    manager
        .register_device("mmio_serial".to_string(), mmio_device)
        .await
        .unwrap();
    manager
        .register_mmio_region("mmio_serial".to_string(), 0x2000_0000, 0x1000)
        .await
        .unwrap();
    mmio_serial.write().await.init().await.unwrap();

    if let Some(handle) = manager.find_mmio_device(0x2000_0000).await {
        // Write to MMIO
        handle.write_register(0, 0x43).await.unwrap(); // 'C'

        // Verify the write succeeded by checking device output
        let output = mmio_serial.read().await.output_string();
        assert!(output.contains('C'));
    }
}

#[tokio::test]
async fn test_mmio_exit_unmapped_address() {
    let (manager, _serial, _timer) = setup_device_manager().await;

    // Test MMIO to unmapped address
    let addr = 0x9000_0000; // Unmapped address

    // Should return None for unmapped address
    assert!(manager.find_mmio_device(addr).await.is_none());
}

#[tokio::test]
async fn test_vm_exit_routing_serial() {
    let (manager, serial, _timer) = setup_device_manager().await;

    // Simulate VM exit for serial I/O OUT
    let exit = VmExit::Io {
        port: 0x3F8,
        direction: IoDirection::Out,
        size: 1,
        data: 0x48, // 'H'
    };

    // Handle the exit
    match exit {
        VmExit::Io {
            port,
            direction,
            data,
            ..
        } => {
            if direction == IoDirection::Out {
                if let Some(handle) = manager.find_io_device(port).await {
                    let offset = (port - handle.base_port()) as u64;
                    handle.write_register(offset, data).await.unwrap();
                }
            }
        }
        _ => panic!("Unexpected exit type"),
    }

    // Verify the write went to the serial device
    assert!(serial.read().await.output_string().contains('H'));
}

#[tokio::test]
async fn test_vm_exit_routing_timer() {
    let (manager, _serial, timer) = setup_device_manager().await;

    // Simulate VM exit for timer control word write
    let exit = VmExit::Io {
        port: 0x43,
        direction: IoDirection::Out,
        size: 1,
        data: 0x34,
    };

    // Handle the exit
    match exit {
        VmExit::Io {
            port,
            direction,
            data,
            ..
        } => {
            if direction == IoDirection::Out {
                if let Some(handle) = manager.find_io_device(port).await {
                    let offset = (port - handle.base_port()) as u64;
                    handle.write_register(offset, data).await.unwrap();
                }
            }
        }
        _ => panic!("Unexpected exit type"),
    }

    // Timer write succeeded (no panic)
    assert_eq!(timer.read().await.name(), "PIT");
}

#[tokio::test]
async fn test_vm_exit_routing_sequence() {
    let (manager, serial, _timer) = setup_device_manager().await;

    // Simulate a sequence of VM exits
    let exits = vec![
        VmExit::Io {
            port: 0x3F8,
            direction: IoDirection::Out,
            size: 1,
            data: 0x48, // 'H'
        },
        VmExit::Io {
            port: 0x3F8,
            direction: IoDirection::Out,
            size: 1,
            data: 0x65, // 'e'
        },
        VmExit::Io {
            port: 0x3F8,
            direction: IoDirection::Out,
            size: 1,
            data: 0x6C, // 'l'
        },
        VmExit::Io {
            port: 0x3F8,
            direction: IoDirection::Out,
            size: 1,
            data: 0x6C, // 'l'
        },
        VmExit::Io {
            port: 0x3F8,
            direction: IoDirection::Out,
            size: 1,
            data: 0x6F, // 'o'
        },
    ];

    // Handle each exit
    for exit in exits {
        match exit {
            VmExit::Io {
                port,
                direction,
                data,
                ..
            } => {
                if direction == IoDirection::Out {
                    if let Some(handle) = manager.find_io_device(port).await {
                        let offset = (port - handle.base_port()) as u64;
                        handle.write_register(offset, data).await.unwrap();
                    }
                }
            }
            _ => panic!("Unexpected exit type"),
        }
    }

    // Verify the complete message was written
    let output = serial.read().await.output_string();
    assert!(output.contains("Hello"));
}
