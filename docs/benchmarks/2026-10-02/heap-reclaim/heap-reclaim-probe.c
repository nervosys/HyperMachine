/* Diagnostic only: load into an owned glibc daemon with an inherited socket.
 * Calls malloc_trim on a dedicated normal thread, never in a signal handler.
 * Both intervention and sham fixtures load this same helper. */
#define _GNU_SOURCE
#include <errno.h>
#include <malloc.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

static int probe_fd = -1;

static long long nanoseconds(struct timespec value) {
    return (long long)value.tv_sec * 1000000000LL + value.tv_nsec;
}

static void *probe(void *unused) {
    (void)unused;
    char command;
    while (recv(probe_fd, &command, 1, 0) == 1) {
        if (command != 'T' && command != 'N') break;
        struct timespec before, after;
        if (clock_gettime(CLOCK_MONOTONIC, &before) != 0) break;
        int result = command == 'T' ? malloc_trim(0) : 0;
        if (clock_gettime(CLOCK_MONOTONIC, &after) != 0) break;
        char reply[192];
        int length = snprintf(reply, sizeof(reply),
            "{\"operation\":\"%s\",\"result\":%d,\"duration_ns\":%lld}\n",
            command == 'T' ? "trim" : "sham", result,
            nanoseconds(after) - nanoseconds(before));
        if (length < 0 || (size_t)length >= sizeof(reply)) break;
        size_t sent = 0;
        while (sent < (size_t)length) {
            ssize_t count = send(probe_fd, reply + sent, (size_t)length - sent, MSG_NOSIGNAL);
            if (count < 0 && errno == EINTR) continue;
            if (count <= 0) goto done;
            sent += (size_t)count;
        }
    }
done:
    close(probe_fd);
    return NULL;
}

__attribute__((constructor)) static void initialize(void) {
    const char *value = getenv("HM_MEMORY_PROBE_FD");
    if (!value) return;
    char *end = NULL;
    errno = 0;
    long descriptor = strtol(value, &end, 10);
    if (errno || end == value || *end || descriptor < 3 || descriptor > 1048576) return;
    probe_fd = (int)descriptor;
    pthread_attr_t attributes;
    if (pthread_attr_init(&attributes) != 0) return;
    if (pthread_attr_setdetachstate(&attributes, PTHREAD_CREATE_DETACHED) != 0) {
        pthread_attr_destroy(&attributes);
        return;
    }
    pthread_t thread;
    if (pthread_create(&thread, &attributes, probe, NULL) != 0) close(probe_fd);
    pthread_attr_destroy(&attributes);
}
