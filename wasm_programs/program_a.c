#include <stdio.h>
#include <unistd.h>
#include <errno.h>

int main() {
    // printf("Program B: Starting and sleeping for 1 second...\n");
    // sleep(10000);
    // printf("Program B: Woke up!\n");
    // return 0;
    printf("Program A: Starting and attempting to read...\n");
    char buffer[128];
    // In a real blocking call this would block—but for our simulation,
    // assume that the WASM shim returns a special value to signal blocking.
    ssize_t n = read(0, buffer, sizeof(buffer));
    if (n < 0) {
        perror("Program A: read");
        // Return 1 to indicate “blocked”
        return 1;
    }
    printf("Program A: Read %zd bytes: %.*s\n", n, (int)n, buffer);
    return 0;
}