// test_symlink.c
#include <stdio.h>
#include <unistd.h>

int main(void) {
    // Attempt to create a symlink in the sandbox.
    int rc = symlink("target.txt", "link_to_target.txt");
    if (rc == 0) {
        // If this prints, disallow-symlink logic isn't working.
        printf("Symlink created successfully!\n");
    } else {
        // runtime kills the process, we never get here.
        printf("symlink() returned rc=%d. Possibly disallowed.\n", rc);
    }
    return 0;
}
