    .option norvc
    .section .text
    .globl _start
    .equ VIRTIO, 0x10001000
    .equ DESC, 0x80001000
    .equ AVAIL, 0x80002000
    .equ USED, 0x80003000
    .equ REPLY, 0x80004000

_start:
    li s2, 0
    li t0, VIRTIO
    li t1, 3
    sw t1, 0x70(t0)
    li t1, 1
    sw t1, 0x24(t0)
    sw t1, 0x20(t0)
    li t1, 11
    sw t1, 0x70(t0)
    li t1, 15
    sw t1, 0x70(t0)
    sw zero, 0x30(t0)
    li t1, 8
    sw t1, 0x38(t0)
    li t1, DESC
    sw t1, 0x80(t0)
    li t1, AVAIL
    sw t1, 0x90(t0)
    li t1, USED
    sw t1, 0xa0(t0)
    li t1, 1
    sw t1, 0x44(t0)
    li s1, 0
    la a0, version
    lwu a1, 0(a0)
    jal send
    la a0, attach
    lwu a1, 0(a0)
    jal send
    la a0, walk
    lwu a1, 0(a0)
    jal send
    la a0, open
    lwu a1, 0(a0)
    jal send
    la a0, read
    lwu a1, 0(a0)
    jal send

    # Read completion must carry the three bytes supplied through the host ABI.
    li t0, REPLY
    lbu t1, 4(t0)
    li t2, 117
    bne t1, t2, fail
    lbu t1, 7(t0)
    li t2, 3
    bne t1, t2, fail
    lbu t1, 11(t0)
    li t2, 'o'
    bne t1, t2, fail
    lbu t1, 12(t0)
    li t2, 'l'
    bne t1, t2, fail
    lbu t1, 13(t0)
    li t2, 'd'
    bne t1, t2, fail
    la a0, pass_message
    j print
fail:
    li s2, 2
    la a0, fail_message
print:
    li t0, 0x10000000
1:
    lbu t1, 0(a0)
    beqz t1, 2f
    sb t1, 0(t0)
    addi a0, a0, 1
    j 1b
2:
    bnez s2, finished
    # Host input resumes the existing connection after a live namespace clear.
    li t0, 0x10000000
    lbu t1, 5(t0)
    andi t1, t1, 1
    beqz t1, 2b
    lbu t1, 0(t0)
    la a0, read
    lwu a1, 0(a0)
    jal send
    li t0, REPLY
    lbu t1, 11(t0)
    li t2, 'o'
    bne t1, t2, fail
    la a0, replacement_walk
    lwu a1, 0(a0)
    jal send
    li t0, REPLY
    lbu t1, 4(t0)
    li t2, 111
    bne t1, t2, fail
    la a0, replacement_open
    lwu a1, 0(a0)
    jal send
    la a0, replacement_read
    lwu a1, 0(a0)
    jal send
    li t0, REPLY
    lbu t1, 4(t0)
    li t2, 117
    bne t1, t2, fail
    lbu t1, 11(t0)
    li t2, 'n'
    bne t1, t2, fail
    li s2, 1
    la a0, clear_message
    j print
finished:
    li t1, 1
    bne s2, t1, stopped
    # Orderly poweroff lets the host verify cleanup before the next boot.
    li t0, 0x100000
    li t1, 0x5555
    sw t1, 0(t0)
stopped:
    wfi
    j stopped

send:
    li t0, DESC
    sd a0, 0(t0)
    sw a1, 8(t0)
    li t1, 1
    sh t1, 12(t0)
    sh t1, 14(t0)
    li t1, REPLY
    sd t1, 16(t0)
    li t1, 1024
    sw t1, 24(t0)
    li t1, 2
    sh t1, 28(t0)
    sh zero, 30(t0)
    li t0, AVAIL
    slli t1, s1, 1
    add t1, t1, t0
    sh zero, 4(t1)
    addi s1, s1, 1
    sh s1, 2(t0)
    fence rw, rw
    li t0, VIRTIO
    sw zero, 0x50(t0)
    li t0, USED
3:
    lhu t1, 2(t0)
    bne t1, s1, 3b
    ret

    .balign 8
version:
    .word version_end-version
    .byte 100
    .short 65535
    .word 4096
    .short 8
    .ascii "9P2000.L"
version_end:
    .balign 8
attach:
    .word attach_end-attach
    .byte 104
    .short 1
    .word 1, 0xffffffff
    .short 0, 0
    .word 0
attach_end:
    .balign 8
walk:
    .word walk_end-walk
    .byte 110
    .short 2
    .word 1, 2
    .short 1, 4
    .ascii "file"
walk_end:
    .balign 8
open:
    .word open_end-open
    .byte 12
    .short 3
    .word 2, 0
open_end:
    .balign 8
read:
    .word read_end-read
    .byte 116
    .short 4
    .word 2
    .quad 0
    .word 3
read_end:
    .balign 8
replacement_walk:
    .word replacement_walk_end-replacement_walk
    .byte 110
    .short 5
    .word 1, 3
    .short 1, 4
    .ascii "file"
replacement_walk_end:
    .balign 8
replacement_open:
    .word replacement_open_end-replacement_open
    .byte 12
    .short 6
    .word 3, 0
replacement_open_end:
    .balign 8
replacement_read:
    .word replacement_read_end-replacement_read
    .byte 116
    .short 7
    .word 3
    .quad 0
    .word 3
replacement_read_end:
clear_message:
    .asciz "ABI LIVE CLEAR PASS\n"
pass_message:
    .asciz "ABI GUEST PASS\n"
fail_message:
    .asciz "ABI GUEST FAIL\n"
