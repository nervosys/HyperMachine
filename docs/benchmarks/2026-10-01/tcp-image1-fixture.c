/* Local-only service for e2e-tcp-tunnel.py; not launched by normal images. */
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>

static int write_all(int fd, const void *bytes, size_t length) {
    const unsigned char *p = bytes;
    while (length) {
        ssize_t n = write(fd, p, length);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) return -1;
        p += n; length -= (size_t)n;
    }
    return 0;
}

static void serve(int fd, int mode) {
    struct timeval timeout = {30, 0};
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));
    unsigned char *bytes = malloc(4 * 1024 * 1024);
    if (!bytes) _exit(2);
    size_t used = 0;
    if (mode == 1) {
        const unsigned char ready[] = {'r','e','a','d','y',0,255};
        if (write_all(fd, ready, sizeof(ready))) _exit(3);
        shutdown(fd, SHUT_WR);
    }
    for (;;) {
        if (used == 4 * 1024 * 1024) _exit(4);
        ssize_t n = read(fd, bytes + used, 4 * 1024 * 1024 - used);
        if (n < 0 && errno == EINTR) continue;
        if (n < 0) _exit(5);
        if (n == 0) break;
        if (mode == 2 && write_all(fd, bytes + used, (size_t)n)) _exit(6);
        used += (size_t)n;
    }
    if (mode == 0 && write_all(fd, bytes, used)) _exit(7);
    if (mode == 1) {
        int output = open("/tmp/tcp-reverse.bin", O_WRONLY | O_CREAT | O_TRUNC, 0600);
        if (output < 0 || write_all(output, bytes, used)) _exit(8);
        close(output);
    }
    shutdown(fd, SHUT_WR);
    free(bytes); close(fd); _exit(0);
}

int main(void) {
    signal(SIGPIPE, SIG_IGN); signal(SIGCHLD, SIG_IGN);
    struct pollfd listeners[4];
    for (int i = 0; i < 4; ++i) {
        int fd = socket(AF_INET, SOCK_STREAM, 0);
        int reuse = 1; setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse));
        struct sockaddr_in address = {.sin_family = AF_INET, .sin_port = htons(18080 + i)};
        address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        if (fd < 0 || bind(fd, (struct sockaddr *)&address, sizeof(address)) || listen(fd, 32)) return 1;
        listeners[i] = (struct pollfd){.fd = fd, .events = POLLIN};
    }
    int ready = open("/tmp/tcp-fixture-ready", O_WRONLY | O_CREAT | O_TRUNC, 0600);
    if (ready < 0 || write_all(ready, "ready", 5)) return 2;
    close(ready);
    for (;;) {
        if (poll(listeners, 4, -1) < 0) { if (errno == EINTR) continue; return 3; }
        for (int i = 0; i < 4; ++i) {
            if (!(listeners[i].revents & POLLIN)) continue;
            int fd = accept(listeners[i].fd, NULL, NULL);
            if (fd < 0) continue;
            pid_t pid = fork();
            if (pid == 0) {
                for (int j = 0; j < 4; ++j) close(listeners[j].fd);
                serve(fd, i);
            }
            close(fd);
        }
    }
}
