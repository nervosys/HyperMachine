import subprocess
for n in [2,3]:
    print('Independent unchanged-gate cohort',n,flush=True)
    r=subprocess.run(['python3','/var/tmp/hm-mmio-irq-order-resume-driver-v'+str(n)+'.py'])
    if r.returncode:
        print('Repeat failure; no retry and no pooled passing claim.',flush=True)
        raise SystemExit(r.returncode)
print('Two independent unchanged-gate repeats terminal success.',flush=True)
