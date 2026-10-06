/* Owned guest private UDP verifier, never part of the production guest agent. */
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>
int main(int argc, char **argv) {
    if (argc != 4 || (strcmp(argv[1], "--probe") && strcmp(argv[1], "--refuse"))) return 2;
    int refuse = !strcmp(argv[1], "--refuse");
    char *end = NULL; long port = strtol(argv[3], &end, 10);
    if (!end || *end || port < 1 || port > 65535) return 2;
    struct sockaddr_in target = {.sin_family=AF_INET, .sin_port=htons((unsigned short)port)};
    if (inet_pton(AF_INET, argv[2], &target.sin_addr) != 1) return 2;
    int fd = socket(AF_INET, SOCK_DGRAM, 0); if (fd < 0) return 3;
    struct timeval timeout = {.tv_sec=refuse ? 2 : 5};
    if (setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout)) || connect(fd, (struct sockaddr *)&target, sizeof(target))) {close(fd);return 3;}
    unsigned char data[1280], received[1281];
    for (size_t i=0;i<sizeof(data);i++) data[i]=(unsigned char)(i%251);
    const size_t sizes[] = {0,13,1280};
    for (size_t n=0;n<(refuse ? 1 : 3);n++) {
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
    close(fd);puts("{\"payload_bytes\":[0,13,1280],\"exact_datagrams\":true}");return 0;
}
