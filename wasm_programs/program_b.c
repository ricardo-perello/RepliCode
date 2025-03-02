// program_b.c
#include <stdio.h>
#include <unistd.h>
extern void __builtin_rt_yield(void);

int main() {
    printf("Program B: Before sleep\n");
    fflush(stdout);
    // Sleep for 1 second (this should map to a poll_oneoff block in your runtime)
    sleep(2);
    printf("Program B: After sleep\n");
    fflush(stdout);
    __builtin_rt_yield();
    printf("executing again");
    return 0;
}
