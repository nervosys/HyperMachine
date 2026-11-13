### repo

| files | agent s median [min-max] | shell s median [min-max] | ratio gross | agent net s | shell net s | ratio net | agent ms/op (est.) | agent ok/runs | shell ok/runs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 100 | 0.74 [0.70-1.63] | 0.37 [0.19-1.86] | 2.0x | 0.08 | -0.32 | -0.2x | 0.4 | 3/3 | 3/3 |
| 1000 | 6.02 [4.42-19.40] | 1.18 [0.83-8.25] | 5.1x | 5.36 | 0.49 | 10.8x | 2.6 | 3/3 | 3/3 |
| 5000 | 28.36 [20.20-31.58] | 3.64 [3.03-17.80] | 7.8x | 27.70 | 2.95 | 9.4x | 2.7 | 3/3 | 3/3 |

### archive

| files | agent s median [min-max] | shell s median [min-max] | ratio gross | agent net s | shell net s | ratio net | agent ms/op (est.) | agent ok/runs | shell ok/runs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 100 | 0.47 [0.29-0.54] | 0.23 [0.11-0.99] | 2.1x | n/a | n/a | n/a | 4.4 | 3/3 | 3/3 |
| 1000 | 5.23 [4.51-11.08] | 0.36 [0.36-3.31] | 14.3x | n/a | n/a | n/a | 5.1 | 3/3 | 3/3 |
| 5000 | 15.34 [10.31-25.75] | 0.92 [0.84-2.77] | 16.7x | n/a | n/a | n/a | 3.0 | 3/3 | 3/3 |

### copy

| files | agent s median [min-max] | shell s median [min-max] | ratio gross | agent net s | shell net s | ratio net | agent ms/op (est.) | agent ok/runs | shell ok/runs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 100 | 0.83 [0.51-1.15] | 0.62 [0.41-1.56] | 1.3x | 0.14 | 0.10 | 1.4x | 0.4 | 2/3 | 3/3 |
| 1000 | 6.81 [6.23-7.66] | 1.12 [0.94-11.89] | 6.1x | 6.12 | 0.61 | 10.0x | 2.0 | 3/3 | 3/3 |
| 5000 | 37.32 [30.21-51.36] | 4.50 [1.89-5.72] | 8.3x | 36.63 | 3.99 | 9.2x | 2.4 | 3/3 | 3/3 |

Errors, copy/100 agent: ["sandbox from the template answered 503: {'code': 503, 'message': 'guest never became ready: Timeout: no answer from a guest agent on vsock port 1024 within 15s (0 refused). The guest may not be running, may have no vsock driver, or may not be running hv2-guest-agentd; the guest wrote nothing to its console; VM Running; vCPU 0: 0 exits (0 "]

