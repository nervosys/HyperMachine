import fcntl
import hashlib
import json
import os
import struct
from pathlib import Path

os.sched_setaffinity(0, {min(os.sched_getaffinity(0))})
fd = os.open('/dev/kvm', os.O_RDWR)
try:
    data = bytearray(8 + 256 * 40)
    struct.pack_into('<II', data, 0, 256, 0)
    fcntl.ioctl(fd, 0xc008ae05, data, True)
    count = struct.unpack_from('<I', data, 0)[0]
    rows = []
    for n in range(count):
        function, index, flags, eax, ebx, ecx, edx = struct.unpack_from('<7I', data, 8 + n * 40)
        if function in (1, 0xb, 0x1f, 0x80000008, 0x8000001e):
            rows.append(dict(function=function, index=index, flags=flags, eax=eax, ebx=ebx, ecx=ecx, edx=edx))
    result = dict(operation='KVM_GET_SUPPORTED_CPUID', affinity=sorted(os.sched_getaffinity(0)), entries=count, topology=rows, source_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
    Path('/var/tmp/hm-competitive/supported-topology-cpu0.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result))
finally:
    os.close(fd)
