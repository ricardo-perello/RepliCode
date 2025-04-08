// test_unlink.c
#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>
#include <string.h>

int main(void) {
    // 1) Create a file
    int fd = open("testfile.txt", O_WRONLY | O_CREAT, 0666);
    if (fd < 0) {
        printf("Failed to create testfile.txt\n");
        return 1;
    }
    const char *msg = "Hello from test_unlink!\n";
    write(fd, msg, strlen(msg));
    close(fd);

    // 2) Unlink the file
    int rc = unlink("testfile.txt");
    if (rc == 0) {
        printf("Unlink succeeded!\n");
    } else {
        printf("Unlink failed! rc=%d\n", rc);
    }

    return 0;
}
