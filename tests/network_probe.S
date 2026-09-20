    .option norvc
    .section .text
    .globl _start

    .equ VIRTIO, 0x10001000
    .equ UART, 0x10000000
    .equ TX_DESC, 0x80001000
    .equ TX_AVAIL, 0x80002000
    .equ TX_USED, 0x80003000
    .equ TX_DATA, 0x80004000
    .equ RX_DESC, 0x80005000
    .equ RX_AVAIL, 0x80006000
    .equ RX_USED, 0x80007000
    .equ RX_DATA, 0x80008000

_start:
    li t0, VIRTIO
    li t1, 1
    sw t1, 0x70(t0)
    li t1, 3
    sw t1, 0x70(t0)

    li t1, 0
    sw t1, 0x24(t0)
    li t1, 0x10020
    sw t1, 0x20(t0)
    li t1, 1
    sw t1, 0x24(t0)
    sw t1, 0x20(t0)
    li t1, 11
    sw t1, 0x70(t0)
    li t1, 15
    sw t1, 0x70(t0)

    li a0, TX_DATA
    la a1, tx_packet
    li a2, 26
1:
    lbu a3, 0(a1)
    sb a3, 0(a0)
    addi a0, a0, 1
    addi a1, a1, 1
    addi a2, a2, -1
    bnez a2, 1b

    li a0, TX_DESC
    li a1, TX_DATA
    sd a1, 0(a0)
    li a1, 26
    sw a1, 8(a0)
    sh zero, 12(a0)
    sh zero, 14(a0)
    li a0, TX_AVAIL
    sh zero, 0(a0)
    li a1, 1
    sh a1, 2(a0)
    sh zero, 4(a0)

    li a1, 1
    sw a1, 0x30(t0)
    li a1, 8
    sw a1, 0x38(t0)
    li a1, TX_DESC
    sw a1, 0x80(t0)
    sw zero, 0x84(t0)
    li a1, TX_AVAIL
    sw a1, 0x90(t0)
    sw zero, 0x94(t0)
    li a1, TX_USED
    sw a1, 0xa0(t0)
    sw zero, 0xa4(t0)
    li a1, 1
    sw a1, 0x44(t0)
    fence rw, rw
    sw a1, 0x50(t0)

    li a0, RX_DESC
    li a1, RX_DATA
    sd a1, 0(a0)
    li a1, 2048
    sw a1, 8(a0)
    li a1, 2
    sh a1, 12(a0)
    sh zero, 14(a0)
    li a0, RX_AVAIL
    sh zero, 0(a0)
    li a1, 1
    sh a1, 2(a0)
    sh zero, 4(a0)

    sw zero, 0x30(t0)
    li a1, 8
    sw a1, 0x38(t0)
    li a1, RX_DESC
    sw a1, 0x80(t0)
    sw zero, 0x84(t0)
    li a1, RX_AVAIL
    sw a1, 0x90(t0)
    sw zero, 0x94(t0)
    li a1, RX_USED
    sw a1, 0xa0(t0)
    sw zero, 0xa4(t0)
    li a1, 1
    sw a1, 0x44(t0)
    fence rw, rw
    sw zero, 0x50(t0)

    li a0, 2000000
2:
    li a1, TX_USED
    lhu a2, 2(a1)
    beqz a2, 3f
    li a1, RX_USED
    lhu a2, 2(a1)
    bnez a2, 4f
3:
    addi a0, a0, -1
    bnez a0, 2b
    la a0, fail_message
    j print

4:
    li a0, RX_DATA + 10
    la a1, rx_packet
    li a2, 16
5:
    lbu a3, 0(a0)
    lbu a4, 0(a1)
    bne a3, a4, 6f
    addi a0, a0, 1
    addi a1, a1, 1
    addi a2, a2, -1
    bnez a2, 5b
    la a0, pass_message
    j print
6:
    la a0, fail_message

print:
    li a1, UART
7:
    lbu a2, 0(a0)
    beqz a2, 8f
    sb a2, 0(a1)
    addi a0, a0, 1
    j 7b
8:
    wfi
    j 8b

    .balign 8
tx_packet:
    .byte 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    .byte 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02, 0, 0, 0, 0, 1, 0x88, 0xb5, 0x54, 0x58
rx_packet:
    .byte 0x02, 0, 0, 0, 0, 1, 0x02, 0, 0, 0, 0, 2, 0x88, 0xb5, 0x52, 0x58
pass_message:
    .asciz "NETWORK PASS\n"
fail_message:
    .asciz "NETWORK FAIL\n"
