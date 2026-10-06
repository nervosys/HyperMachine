# Independent serialized-IRQ resume repetitions

The frozen isolated release passes two additional independently started cohorts, each with 20 total main-target resumes, 19 added journal rows, exact cycle-identified UDP, prior-session closure, 21 KVM checks and full cleanup. Same runtime inputs as ../mmio-irq-order/, original 30-second API deadline and no timed retries. Together the three cohorts cover 60 resumes. The checker is preserved and verify.py independently validates both reports and journals.

This supports the verified transport race fix but does not prove the intermittent stall is permanently eliminated or validate the provisional CPU reduction. Earlier acknowledged-pause failures remain recorded. The full traffic performance comparison still needs repeating on the fixed candidate. Logs and driver for each independent cohort are preserved.
