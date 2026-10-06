/* Owned guest private UDP verifier, never part of the production guest agent. */
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>
static double monotonic_seconds(void) {
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now)) exit(3);
    return now.tv_sec + now.tv_nsec / 1000000000.0;
}
static int live_probe(int fd, const char *marker) {
    struct timeval timeout = {.tv_usec=200000};
    if (setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout))) return 3;
    unsigned char data[64], reply[65];
    double start=monotonic_seconds(), revoked=0;
    unsigned before=0, after=0, refused=0, sequence=0;
    while (monotonic_seconds()-start < 20) {
        if (!revoked && access(marker,F_OK)==0) revoked=monotonic_seconds();
        int past_grace=revoked && monotonic_seconds()-revoked>=3;
        for (size_t i=0;i<sizeof(data);i++) data[i]=(unsigned char)(i%251);
        unsigned network_sequence=htonl(++sequence);memcpy(data,&network_sequence,sizeof(network_sequence));
        ssize_t sent=send(fd,data,sizeof(data),0);
        ssize_t count=sent==(ssize_t)sizeof(data) ? recv(fd,reply,sizeof(reply),0) : -1;
        int exact=count==(ssize_t)sizeof(data) && !memcmp(data,reply,sizeof(data));
        if (count>=0 && !exact) return 5;
        if (count<0 && errno!=EAGAIN && errno!=EWOULDBLOCK && errno!=ECONNREFUSED) return 5;
        if (exact) {
            if (past_grace) return 5;
            if (revoked) after++; else before++;
            if (!revoked && before==1) {puts("{\"ready\":true}");fflush(stdout);}
        } else if (!revoked && !before) return 5;
        else if (past_grace && ++refused==3) {
            printf("{\"revoked\":true,\"same_socket\":true,\"replies_before\":%u,\"grace_replies\":%u,\"post_grace_refusals\":%u}\n",before,after,refused);
            return before ? 0 : 5;
        }
        usleep(100000);
    }
    return 5;
}
int main(int argc, char **argv) {
    int live = argc==5 && !strcmp(argv[1], "--live");
    if (!live && (argc != 4 || (strcmp(argv[1], "--probe") && strcmp(argv[1], "--refuse")))) return 2;
    int refuse = !strcmp(argv[1], "--refuse");
    char *end = NULL; long port = strtol(argv[3], &end, 10);
    if (!end || *end || port < 1 || port > 65535) return 2;
    struct sockaddr_in target = {.sin_family=AF_INET, .sin_port=htons((unsigned short)port)};
    if (inet_pton(AF_INET, argv[2], &target.sin_addr) != 1) return 2;
    int fd = socket(AF_INET, SOCK_DGRAM, 0); if (fd < 0) return 3;
    struct timeval timeout = {.tv_sec=refuse ? 2 : 5};
    if (setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout)) || connect(fd, (struct sockaddr *)&target, sizeof(target))) {close(fd);return 3;}
    int fragmentation = IP_PMTUDISC_DONT;
    if (setsockopt(fd, IPPROTO_IP, IP_MTU_DISCOVER, &fragmentation, sizeof(fragmentation))) {close(fd);return 3;}
    if (live) {int result=live_probe(fd,argv[4]);close(fd);return result;}
    unsigned char data[65507], received[65508];
    for (size_t i=0;i<sizeof(data);i++) data[i]=(unsigned char)(i%251);
    const size_t sizes[] = {0,13,1280,65507};
    for (size_t n=0;n<(refuse ? 1 : sizeof(sizes)/sizeof(sizes[0]));n++) {
        size_t size = refuse ? 13 : sizes[n];
        if (send(fd,data,size,0) != (ssize_t)size) {close(fd);return 4;}
        ssize_t count = recv(fd,received,sizeof(received),0);
        if (refuse) {
            int denied=count<0 && (errno==EAGAIN || errno==EWOULDBLOCK || errno==ECONNREFUSED);
            close(fd);if (!denied) return 5;
            puts("{\"refused\":true}");return 0;
        }
        if (count != (ssize_t)size || memcmp(data,received,size)) {close(fd);return 5;}
    }
    close(fd);puts("{\"payload_bytes\":[0,13,1280,65507],\"exact_datagrams\":true}");return 0;
}
