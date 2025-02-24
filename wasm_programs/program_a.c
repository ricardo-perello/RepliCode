// program_a.c
#include <stdio.h>
#include <unistd.h>
#include <errno.h>

int main() {
    printf("Program A: Starting and attempting first read...\n");
    fflush(stdout);
    char buffer[128];
    ssize_t n = read(0, buffer, sizeof(buffer));
    if (n < 0) {
        perror("Program A: first read");
        return 1;
    }
    printf("Program A: First read %zd bytes: %.*s\n", n, (int)n, buffer);
    fflush(stdout);

    printf("Program A: Attempting second read...\n");
    fflush(stdout);
    n = read(0, buffer, sizeof(buffer));
    if (n < 0) {
        perror("Program A: second read");
        return 1;
    }
    printf("Program A: Second read %zd bytes: %.*s\n", n, (int)n, buffer);
    fflush(stdout);
    return 0;
}
