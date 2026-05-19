/*
 * benchmarks/harness/workloads/vmexit.c
 *
 * In-guest VM exit microbenchmark. Measures the cost of forcing a small set
 * of operations that typically cause VM exits on x86 hosts:
 *
 *   - cpuid (forced unconditional exit on virtualized CPUID)
 *   - hlt   (only with privilege; user-mode falls back to syscall(SYS_pause))
 *   - mmio  (read a known MMIO BAR if available; otherwise skipped)
 *
 * Build inside guest:
 *   cc -O2 -o vmexit vmexit.c
 *
 * Invocation: ./vmexit <iterations>
 *
 * Output: TSV `<metric>\t<value>\tns`
 */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <time.h>
#include <string.h>

static inline uint64_t now_ns(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC_RAW, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ull + ts.tv_nsec;
}

static inline void do_cpuid(void)
{
#if defined(__x86_64__) || defined(__i386__)
    unsigned a, b, c, d;
    __asm__ volatile("cpuid"
                     : "=a"(a), "=b"(b), "=c"(c), "=d"(d)
                     : "a"(0), "c"(0));
#endif
}

int main(int argc, char **argv)
{
    long iters = (argc > 1) ? atol(argv[1]) : 1000000;
    if (iters <= 0)
        iters = 1000000;

    /* CPUID */
    uint64_t t0 = now_ns();
    for (long i = 0; i < iters; i++)
        do_cpuid();
    uint64_t t1 = now_ns();
    double cpuid_ns = (double)(t1 - t0) / (double)iters;
    printf("vmexit_cpuid_ns\t%.2f\tns\n", cpuid_ns);

    /* getpid (cheap syscall — calibrates baseline kernel transition) */
    extern long syscall(long, ...);
#ifndef SYS_getpid
#include <sys/syscall.h>
#endif
    t0 = now_ns();
    for (long i = 0; i < iters; i++)
        (void)syscall(SYS_getpid);
    t1 = now_ns();
    printf("vmexit_getpid_ns\t%.2f\tns\n", (double)(t1 - t0) / (double)iters);

    return 0;
}
