/* Separate-process allocation diagnostic; never loaded into a scored daemon. */
#define _GNU_SOURCE
#include <malloc.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void observe(const char *stage, int comma) {
    struct mallinfo2 value = mallinfo2();
    printf("%s{\"stage\":\"%s\",\"arena\":%zu,\"mapped\":%zu,\"used\":%zu,\"free\":%zu}",
           comma ? "," : "", stage, value.arena, value.hblkhd, value.uordblks, value.fordblks);
}

int main(void) {
    const char *threshold = getenv("MALLOC_MMAP_THRESHOLD_");
    if (threshold && strcmp(threshold, "131072") != 0) return 2;
    printf("{\"threshold\":%s,\"diagnostic_only\":true,\"samples\":[", threshold ? "131072" : "null");
    observe("before", 0);
    for (int round = 0; round < 2; ++round) {
        const size_t sizes[2] = {13 * 1024 * 1024, 2 * 1024 * 1024};
        volatile unsigned char *buffers[2];
        for (int index = 0; index < 2; ++index) {
            buffers[index] = malloc(sizes[index]);
            if (!buffers[index]) return 3;
            for (size_t offset = 0; offset < sizes[index]; offset += 4096) buffers[index][offset] = 0x5a;
        }
        observe(round ? "second-live" : "first-live", 1);
        free((void *)buffers[1]);
        free((void *)buffers[0]);
        observe(round ? "second-freed" : "first-freed", 1);
    }
    puts("]}");
    return 0;
}
