// test_escape_sandbox.c
#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>

int main(void) {
    // Attempt to open a file outside the sandbox.
    int fd = open(".../etc/passwd", O_RDONLY);
    if (fd < 0) {
        // Expected: should fail if sandboxing is enforced.
        printf("Attempt to open /etc/passwd failed (as expected). fd=%d\n", fd);
    } else {
        // If this prints, your sandbox check is not working properly.
        printf("ERROR: opened /etc/passwd successfully! fd=%d\n", fd);
        close(fd);
    }
    return 0;
}
