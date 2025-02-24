// program_c.c
#include <stdio.h>
#include <unistd.h>
#include <errno.h>

int main() {
    printf("Program C: Starting and attempting to read (should block until second batch)...\n");
    fflush(stdout);
    char buffer[128];
    ssize_t n = read(0, buffer, sizeof(buffer));
    if (n < 0) {
        perror("Program C: read");
        return 1;
    }
    printf("Program C: Read %zd bytes: %.*s\n", n, (int)n, buffer);
    fflush(stdout);
    return 0;
}
