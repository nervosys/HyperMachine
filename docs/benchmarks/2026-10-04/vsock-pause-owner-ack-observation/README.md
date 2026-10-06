# Acknowledged-pause diagnostic observation

This separate diagnostic changes only the general API client's observation timeout from 30 to 90 seconds. It has no timed retries and uses the same frozen candidate and runtime inputs. It is not an original-gate result.

All 20 main-target resumes completed (19 additional journal rows), with exact identified UDP and prior session closure. The report records 21 KVM checks, zero guests remaining and all owned processes reaped. Resume API completions were all below 30 ms. The intermittent failure was not reproduced, so the longer timeout does not explain it. The preceding first-resume timeout remains in ../vsock-pause-owner-ack/ and prevents accepting a lifecycle fix or provisional CPU gain.

Checker, executed driver, raw report, journal and daemon output are preserved with hashes. A separate unchanged-deadline repeat is tracked independently.
