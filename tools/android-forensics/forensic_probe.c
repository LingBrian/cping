/*
 * 取证对照程序（不属于 cping 仓库，仅放在 analysis/ 下做平台行为验证）
 *
 * 用途：验证 Android/bionic 在两种「致命日志」下的真实行为，用来判定
 *       cping GUI 退后台时那条 FORTIFY 日志到底是崩溃还是噪音。
 *
 *   ./forensic_probe abort      —— 真 abort()：预期产生 tombstone + Fatal signal 6
 *   ./forensic_probe mutex      —— 在已 destroy 的默认互斥量上 lock
 *   ./forensic_probe mutex-ec   —— 在已 destroy 的 ERRORCHECK 互斥量上 lock
 */
#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int do_mutex(int errorcheck) {
    pthread_mutex_t m;
    pthread_mutexattr_t attr;

    if (errorcheck) {
        pthread_mutexattr_init(&attr);
        pthread_mutexattr_settype(&attr, PTHREAD_MUTEX_ERRORCHECK);
        if (pthread_mutex_init(&m, &attr) != 0) {
            printf("init failed: %s\n", strerror(errno));
            return 2;
        }
        pthread_mutexattr_destroy(&attr);
    } else {
        if (pthread_mutex_init(&m, NULL) != 0) {
            printf("init failed: %s\n", strerror(errno));
            return 2;
        }
    }

    pthread_mutex_destroy(&m);
    printf("mutex destroyed (errorcheck=%d), now locking it...\n", errorcheck);
    fflush(stdout);

    int rc = pthread_mutex_lock(&m);
    printf("pthread_mutex_lock returned rc=%d errno=%d (%s)\n", rc, errno,
           strerror(errno));
    fflush(stdout);

    /* 若上面那行能打印出来，说明 FORTIFY 只是记了日志、没有终止进程 */
    if (rc == 0) {
        pthread_mutex_unlock(&m);
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc < 2) {
        printf("usage: %s abort|mutex|mutex-ec\n", argv[0]);
        return 2;
    }

    if (strcmp(argv[1], "abort") == 0) {
        printf("about to call abort()...\n");
        fflush(stdout);
        abort(); /* 预期：SIGABRT + tombstone */
        return 1; /* 不可达 */
    }
    if (strcmp(argv[1], "mutex") == 0) {
        return do_mutex(0);
    }
    if (strcmp(argv[1], "mutex-ec") == 0) {
        return do_mutex(1);
    }

    printf("unknown mode: %s\n", argv[1]);
    return 2;
}
