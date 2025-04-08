// test_disk_limit.c
#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>
#include <string.h>

int main(void) {
    // Create or open a file for writing.
    int fd = open("bigfile.txt", O_WRONLY | O_CREAT, 0666);
    if (fd < 0) {
        printf("Failed to create bigfile.txt\n");
        return 1;
    }

    // Write data in a loop until we (hopefully) exceed the disk limit.
    const char *buf = "Hello, writing more than the disk limit...\n";
    size_t len = strlen(buf);

    for (int i = 0; i < 10000; i++) {
        ssize_t written = write(fd, buf, len);
        if (written < 0) {
            printf("Write failed at iteration %d\n", i);
            close(fd);
            return 1;
        }
        // Print progress once in a while.
        if (i % 1000 == 0) {
            printf("Wrote another chunk (%d) ...\n", i);
        }
    }

    printf("Finished writing without hitting limit?\n");
    close(fd);
    return 0;
}
