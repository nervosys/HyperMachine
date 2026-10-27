import fcntl, os, struct, sys

# KVM_GET_SUPPORTED_CPUID = _IOWR(KVMIO 0xAE, 0x05, struct kvm_cpuid2): the struct header is 8 bytes.
KVM_GET_SUPPORTED_CPUID = 0xC008AE05
ENTRY = struct.Struct("<10I")  # function, index, flags, eax, ebx, ecx, edx, padding[3]
N = 256

fd = os.open("/dev/kvm", os.O_RDWR)
buf = bytearray(8 + N * ENTRY.size)
struct.pack_into("<II", buf, 0, N, 0)
fcntl.ioctl(fd, KVM_GET_SUPPORTED_CPUID, buf)
nent = struct.unpack_from("<I", buf, 0)[0]
leaves = {}
for i in range(nent):
    f, idx, flags, eax, ebx, ecx, edx, *_ = ENTRY.unpack_from(buf, 8 + i * ENTRY.size)
    leaves.setdefault(f, []).append((idx, eax, ebx, ecx, edx))
print("kernel", os.uname().release, "entries", nent)
print("has leaf 0x40000000:", 0x40000000 in leaves, "| has leaf 0x40000001:", 0x40000001 in leaves)
if 0x40000000 in leaves:
    _, eax, ebx, ecx, edx = leaves[0x40000000][0]
    print("0x40000000 max=%#x signature=%r" % (eax, struct.pack("<III", ebx, ecx, edx)))
if 0x40000001 in leaves:
    _, eax, ebx, ecx, edx = leaves[0x40000001][0]
    print("0x40000001 eax=%#x (CLOCKSOURCE2 bit3=%d, CLOCKSOURCE bit0=%d, STABLE_TSC bit24=%d)" % (eax, eax >> 3 & 1, eax & 1, eax >> 24 & 1))
if 1 in leaves:
    _, eax, ebx, ecx, edx = leaves[1][0]
    print("leaf 1 ecx=%#x HYPERVISOR bit31=%d" % (ecx, ecx >> 31 & 1))
print("leaves present:", " ".join("%#x" % f for f in sorted(leaves) if f >= 0x40000000 and f < 0x50000000) or "no 0x4xxxxxxx leaves")
