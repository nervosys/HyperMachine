/* Owned loopback echo service used only by the UDP KVM verification fixture. */
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>
#include <sys/socket.h>
#include <unistd.h>

static int echo_port = 5353;
static int tag_port;
static int receive_buffer;

static int configure_receive_buffer(int fd) {
    if (!receive_buffer) return 0;
    /* Owned root fixture: explicit bounded buffering for burst workloads. */
    if (setsockopt(fd, SOL_SOCKET, SO_RCVBUFFORCE, &receive_buffer, sizeof(receive_buffer)) < 0) return -1;
    int actual; socklen_t size = sizeof(actual);
    if (getsockopt(fd, SOL_SOCKET, SO_RCVBUF, &actual, &size) < 0 || actual < receive_buffer) return -1;
    fprintf(stderr, "{\"requested_receive_buffer\":%d,\"actual_receive_buffer\":%d}\n", receive_buffer, actual);
    return 0;
}

/* Optional destination identity for capacity fixtures; normal echo is unchanged. */
static ssize_t send_reply(int fd, const unsigned char *payload, ssize_t length,
                         const struct sockaddr *peer, socklen_t size) {
    if (!tag_port) return sendto(fd, payload, length, 0, peer, size);
    unsigned char response[65507];
    response[0] = (unsigned char)(echo_port >> 8);
    response[1] = (unsigned char)echo_port;
    memcpy(response + 2, payload, (size_t)length);
    return sendto(fd, response, (size_t)length + 2, 0, peer, size);
}

static int probe_ipv6(void) {
    int receiver = -1, sender = -1, status = 1, fragmented = 0;
    int enabled = 1;
    struct timeval timeout = { .tv_sec = 2 };
    struct sockaddr_in6 local = { .sin6_family = AF_INET6,
        .sin6_addr = IN6ADDR_LOOPBACK_INIT };
    struct sockaddr_in6 target, source, sent_from;
    socklen_t address_size = sizeof(target);
    unsigned char payload[65507], reply[65508];
    const size_t sizes[] = { 0, 4, 65507 };
    for (size_t n = 0; n < sizeof(payload); n++) payload[n] = (unsigned char)(n % 251);
    receiver = socket(AF_INET6, SOCK_DGRAM, 0);
    sender = socket(AF_INET6, SOCK_DGRAM, 0);
    if (receiver < 0 || sender < 0) goto done;
    if (setsockopt(receiver, IPPROTO_IPV6, IPV6_V6ONLY, &enabled, sizeof(enabled)) < 0 ||
        setsockopt(sender, IPPROTO_IPV6, IPV6_V6ONLY, &enabled, sizeof(enabled)) < 0 ||
        setsockopt(receiver, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout)) < 0 ||
        bind(receiver, (struct sockaddr *)&local, sizeof(local)) < 0 ||
        bind(sender, (struct sockaddr *)&local, sizeof(local)) < 0 ||
        getsockname(receiver, (struct sockaddr *)&target, &address_size) < 0) goto done;
    address_size = sizeof(sent_from);
    if (getsockname(sender, (struct sockaddr *)&sent_from, &address_size) < 0) goto done;
    for (size_t n = 0; n < sizeof(sizes) / sizeof(sizes[0]); n++) {
        ssize_t sent = sendto(sender, payload, sizes[n], 0, (struct sockaddr *)&target, sizeof(target));
        if (sent < 0 && errno == EMSGSIZE && sizes[n] == 65507) {
            int mode = IPV6_PMTUDISC_DONT;
            if (setsockopt(sender, IPPROTO_IPV6, IPV6_MTU_DISCOVER, &mode, sizeof(mode)) < 0) goto done;
            fragmented = 1;
            sent = sendto(sender, payload, sizes[n], 0, (struct sockaddr *)&target, sizeof(target));
        }
        if (sent != (ssize_t)sizes[n]) goto done;
        address_size = sizeof(source);
        ssize_t received = recvfrom(receiver, reply, sizeof(reply), 0,
            (struct sockaddr *)&source, &address_size);
        if (received != (ssize_t)sizes[n] || memcmp(payload, reply, sizes[n]) != 0 ||
            source.sin6_family != AF_INET6 || !IN6_IS_ADDR_LOOPBACK(&source.sin6_addr) ||
            source.sin6_port != sent_from.sin6_port) { errno = EIO; goto done; }
    }
    printf("{\"ipv6_loopback\":true,\"payload_sizes\":[0,4,65507],\"exact_datagrams\":true,"
        "\"max_payload_requires_local_fragmentation\":%s}\n", fragmented ? "true" : "false");
    status = 0;
done:
    if (status != 0) perror("owned IPv6 UDP probe");
    if (sender >= 0) close(sender);
    if (receiver >= 0) close(receiver);
    return status;
}

static int echo_ipv6(void) {
    int fd = socket(AF_INET6, SOCK_DGRAM, 0), enabled = 1;
    if (fd < 0) return 1;
    if (configure_receive_buffer(fd) < 0) return 2;
    struct sockaddr_in6 local = { .sin6_family = AF_INET6,
        .sin6_port = htons(echo_port), .sin6_addr = IN6ADDR_LOOPBACK_INIT };
    if (setsockopt(fd, IPPROTO_IPV6, IPV6_V6ONLY, &enabled, sizeof(enabled)) < 0 ||
        bind(fd, (struct sockaddr *)&local, sizeof(local)) < 0) return 2;
    unsigned char payload[65508];
    for (;;) {
        struct sockaddr_in6 peer;
        socklen_t size = sizeof(peer);
        ssize_t received = recvfrom(fd, payload, sizeof(payload), 0,
            (struct sockaddr *)&peer, &size);
        if (received < 0) { if (errno == EINTR) continue; return 3; }
        if (received > (tag_port ? 65505 : 65507)) continue;
        if (send_reply(fd, payload, received, (struct sockaddr *)&peer, size) != received + (tag_port ? 2 : 0)) return 4;
    }
}

int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "--ipv6-probe") == 0) return probe_ipv6();
    int ipv6 = 0;
    for (int n = 1; n < argc; n++) {
        if (strcmp(argv[n], "--tag-port") == 0) { tag_port = 1; continue; }
        if (strcmp(argv[n], "--ipv6") == 0) { ipv6 = 1; continue; }
        if (strcmp(argv[n], "--receive-buffer") == 0 && n + 1 < argc) {
            char *end; errno = 0;
            long value = strtol(argv[++n], &end, 10);
            if (errno || !*argv[n] || *end || value < 65536 || value > 16777216) return 2;
            receive_buffer = (int)value; continue;
        }
        if (strcmp(argv[n], "--port") == 0 && n + 1 < argc) {
            char *end; errno = 0;
            long port = strtol(argv[++n], &end, 10);
            if (errno || !*argv[n] || *end || port < 1 || port > 65535) return 2;
            echo_port = (int)port; continue;
        }
        return 2;
    }
    if (ipv6) return echo_ipv6();
    int fd = socket(AF_INET, SOCK_DGRAM, 0);
    if (fd < 0) return 1;
    if (configure_receive_buffer(fd) < 0) return 2;
    struct sockaddr_in local = { .sin_family = AF_INET,
        .sin_port = htons(echo_port), .sin_addr.s_addr = htonl(INADDR_LOOPBACK) };
    if (bind(fd, (struct sockaddr *)&local, sizeof(local)) < 0) return 2;
    unsigned char payload[65508];
    for (;;) {
        struct sockaddr_in peer;
        socklen_t size = sizeof(peer);
        ssize_t received = recvfrom(fd, payload, sizeof(payload), 0,
            (struct sockaddr *)&peer, &size);
        if (received < 0) { if (errno == EINTR) continue; return 3; }
        if (received > (tag_port ? 65505 : 65507)) continue;
        if (send_reply(fd, payload, received, (struct sockaddr *)&peer, size) != received + (tag_port ? 2 : 0)) return 4;
    }
}
