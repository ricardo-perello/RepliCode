// program_b.c
#include <stdio.h>
#include <unistd.h>

int main() {
    printf("Program B: Before sleep\n");
    fflush(stdout);
    // Sleep for 1 second (this should map to a poll_oneoff block in your runtime)
    sleep(1);
    printf("Program B: After sleep\n");
    fflush(stdout);
    return 0;
}
