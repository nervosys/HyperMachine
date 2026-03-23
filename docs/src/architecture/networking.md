# Networking

HyperMachine provides a full networking stack for virtual machines, including TAP/TUN device
support, VirtIO-net emulation, and a software virtual switch with VLAN support.

## Virtual Switch

The built-in virtual switch (`vswitch`) provides Layer-2 forwarding between VM ports with
enterprise-grade features:

| Feature            | Description                                                     |
| ------------------ | --------------------------------------------------------------- |
| **MAC Learning**   | Automatic source MAC learning with configurable aging           |
| **VLAN Support**   | Access and trunk port modes, 802.1Q tagging/untagging           |
| **Broadcast**      | Flood to all ports in the same VLAN                             |
| **Multicast**      | Group delivery with promiscuous mode support                    |
| **Port Control**   | Enable/disable ports, per-port statistics                       |
| **MAC Table**      | Configurable capacity with automatic expiry                     |
| **Promiscuous**    | Per-port promiscuous mode for monitoring                        |

### Architecture

```
+------------------+     +------------------+     +------------------+
|     VM 1         |     |     VM 2         |     |     VM 3         |
|  (virtio-net)    |     |  (virtio-net)    |     |  (virtio-net)    |
+--------+---------+     +--------+---------+     +--------+---------+
         |                        |                        |
    +----+----+              +----+----+              +----+----+
    | Port 1  |              | Port 2  |              | Port 3  |
    | Access  |              | Access  |              | Trunk   |
    | VLAN 10 |              | VLAN 10 |              | 10,20   |
    +----+----+              +----+----+              +----+----+
         |                        |                        |
    +----+------------------------+------------------------+----+
    |                    Virtual Switch                          |
    |                                                            |
    |  +------------------+    +--------------------------+      |
    |  | MAC Table        |    | VLAN Database            |      |
    |  | AA:BB:CC → Port1 |    | VLAN 10: Port 1,2,3     |      |
    |  | DD:EE:FF → Port2 |    | VLAN 20: Port 3         |      |
    |  +------------------+    +--------------------------+      |
    +------------------------------------------------------------+
```

### Configuration

VMs are connected to the switch via ports. Each port can operate in access or trunk mode:

```rust
use hv2_core::networking::vswitch::*;
use std::time::Duration;

// Create a switch with 10K MAC table capacity and 5-minute aging
let mut switch = VirtualSwitch::new("br0", 10_000, Duration::from_secs(300));

// Add access ports (single VLAN)
switch.add_port(Port::new(1, "vm1-eth0", PortType::VmPort));
switch.add_port(Port::new(2, "vm2-eth0", PortType::VmPort));

// Add a trunk port for inter-VLAN routing
let mut trunk = Port::new(3, "router-eth0", PortType::Uplink);
// Configure trunk VLANs as needed
switch.add_port(trunk);
```

### Packet Flow

1. **Ingress**: Frame arrives on a port
2. **Learning**: Source MAC is learned and associated with the ingress port
3. **VLAN Check**: Frame's VLAN must be allowed on the ingress port
4. **Lookup**: Destination MAC is looked up in the MAC table
5. **Forwarding**:
   - **Unicast hit**: Forward to the specific learned port
   - **Unicast miss / Broadcast**: Flood to all ports in the same VLAN
   - **Same-port**: Drop (no hairpin)
6. **Egress**: VLAN tag added/removed based on port mode

## TAP/TUN Devices

On Linux, HyperMachine uses TAP devices for VM network connectivity. The TAP subsystem
supports:

- Multi-queue for parallel packet processing
- `vnet_hdr` for offload negotiation
- Automatic MAC address generation
- Non-blocking I/O

## VirtIO-Net

The VirtIO network device provides high-performance paravirtualized networking:

- Separate TX/RX virtqueues
- Multi-queue support for SMP guests
- Feature negotiation (checksum offload, TSO, etc.)
- Interrupt coalescing via used ring notification suppression

## Performance

Benchmark the networking stack:

```bash
cargo bench -p hv2-core --bench vswitch_bench
```

Key benchmarks:
- **MAC table learn**: Insert rate across table sizes (100–10K entries)
- **MAC table lookup**: Lookup latency with varying table occupancy
- **MAC table aging**: Expiry sweep performance
- **VLAN set operations**: Bitmap add/contains throughput
