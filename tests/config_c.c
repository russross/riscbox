#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "machine.c"

const VirtMachineClass riscv_machine_class = { 0 };

#define CHECK(condition) do {                                                \
        if (!(condition)) {                                                  \
            fprintf(stderr, "%s:%d: check failed: %s\n",                    \
                    __FILE__, __LINE__, #condition);                         \
            exit(1);                                                         \
        }                                                                    \
    } while (0)

static int parse(VirtMachineParams *params, const char *source)
{
    virt_machine_set_defaults(params);
    return virt_machine_parse_config(params, (char *)source, strlen(source));
}

static void test_complete_config(void)
{
    static const char source[] =
        "{/*comment*/ version:1,machine:\"riscv64\",memory_size:0x100,"
        "bios:\"fw.bin\",kernel:\"linux\",initrd:\"initrd\","
        "cmdline:\"root=/dev/vda\",console:\"uart\",uart_output:true,"
        "drive0:{file:\"disk\",device:\"virtio\"},"
        "fs0:{file:\"fs/head\"},fs1:{js9p:true,tag:\"shared\"},"
        "eth0:{driver:\"tap\",ifname:\"tap0\"},"
        "display0:{device:\"simplefb\",width:640,height:480},"
        "input_device:\"virtio\",rtc_local_time:true,}";
    VirtMachineParams params;

    CHECK(parse(&params, source) == 0);
    CHECK(params.ram_size == UINT64_C(256) << 20);
    CHECK(params.console_type == VM_CONSOLE_UART);
    CHECK(params.uart_output);
    CHECK(params.drive_count == 1);
    CHECK(!strcmp(params.tab_drive[0].filename, "disk"));
    CHECK(params.fs_count == 2);
    CHECK(!strcmp(params.tab_fs[0].tag, "/dev/root"));
    CHECK(params.tab_fs[1].backend_type == VM_FS_JS9P);
    CHECK(!strcmp(params.tab_fs[1].tag, "shared"));
    CHECK(params.eth_count == 1);
    CHECK(!strcmp(params.tab_eth[0].ifname, "tap0"));
    CHECK(params.width == 640 && params.height == 480);
    CHECK(params.rtc_local_time);
    virt_machine_free_config(&params);
}

static void test_defaults_gaps_and_cmdline(void)
{
    VirtMachineParams params;

    CHECK(parse(&params,
                "{version:1,machine:\"riscv64\",memory_size:128,"
                "drive1:{file:\"ignored\"}}") == 0);
    CHECK(params.console_type == VM_CONSOLE_VIRTIO);
    CHECK(params.drive_count == 0);
    vm_add_cmdline(&params, "console=hvc0");
    CHECK(!strcmp(params.cmdline, " console=hvc0"));
    vm_add_cmdline(&params, "!console=ttyS0");
    CHECK(!strcmp(params.cmdline, "console=ttyS0"));
    virt_machine_free_config(&params);
}

static void test_failures(void)
{
    static const char *const sources[] = {
        "{}",
        "{version:2,machine:\"riscv64\",memory_size:128}",
        "{version:1,machine:\"riscv64\",memory_size:\"128\"}",
        "{version:1,machine:\"riscv64\",memory_size:128,console:\"bad\"}",
        "{version:1,machine:\"riscv64\",memory_size:128,uart_output:1}",
        "{version:1,machine:\"riscv64\",memory_size:128,fs0:{js9p:true,file:\"x\"}}",
        "{version:1,machine:\"riscv64\",memory_size:128,eth0:{driver:\"tap\"}}",
    };
    size_t i;

    for (i = 0; i < sizeof(sources) / sizeof(sources[0]); i++) {
        VirtMachineParams params;
        CHECK(parse(&params, sources[i]) < 0);
        virt_machine_free_config(&params);
    }
}

static void test_paths(void)
{
    char *path;

    path = get_file_path("https://host/vm/riscbox.cfg", "fw.bin");
    CHECK(!strcmp(path, "https://host/vm/fw.bin"));
    free(path);
    path = get_file_path("https://host/vm/riscbox.cfg", "data:image/raw");
    CHECK(!strcmp(path, "data:image/raw"));
    free(path);
    path = get_file_path("https://host/vm/riscbox.cfg", "/fw.bin");
    CHECK(!strcmp(path, "/fw.bin"));
    free(path);
}

int main(void)
{
    test_complete_config();
    test_defaults_gaps_and_cmdline();
    test_failures();
    test_paths();
    puts("configuration C tests passed");
    return 0;
}
