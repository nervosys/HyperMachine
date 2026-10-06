/* Owned loopback echo service used only by the UDP KVM verification fixture. */
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
    int fd = socket(AF_INET, SOCK_DGRAM, 0);
    if (fd < 0) return 1;
    struct sockaddr_in local = { .sin_family = AF_INET,
        .sin_port = htons(5353), .sin_addr.s_addr = htonl(INADDR_LOOPBACK) };
    if (bind(fd, (struct sockaddr *)&local, sizeof(local)) < 0) return 2;
    unsigned char payload[65508];
    for (;;) {
        struct sockaddr_in peer;
        socklen_t size = sizeof(peer);
        ssize_t received = recvfrom(fd, payload, sizeof(payload), 0,
            (struct sockaddr *)&peer, &size);
        if (received < 0) { if (errno == EINTR) continue; return 3; }
        if (received > 65507) continue;
        if (sendto(fd, payload, received, 0, (struct sockaddr *)&peer, size) != received) return 4;
    }
}
