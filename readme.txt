TinyEMU System Emulator by Fabrice Bellard
==========================================

1) Features
-----------

- RISC-V system emulator supporting the RV64IMAFDC base ISA (user
  level ISA version 2.2, priviledged architecture version 1.10)
  including:

  - 64 bit integer registers
  - 32/64 bit floating point instructions
  - Compressed instructions

- 16550A UART and VirtIO console, network, block device, input and 9P filesystem

- Graphical display with SDL

- JSON configuration file

- Remote HTTP block device and filesystem

- small code, easy to modify, no external dependancies

- Javascript demo version

2) Installation
---------------

- The libraries libcurl, OpenSSL and SDL should be installed. On a Fedora
  system you can do it with:

  sudo dnf install openssl-devel libcurl-devel SDL-devel

  It is possible to compile the programs without these libraries by
  commenting CONFIG_FS_NET and/or CONFIG_SDL in the Makefile.

- Use 'make' to compile the binaries.

- You can optionally install the program to '/usr/local/bin' with:

  make install

3) Usage
--------

3.1 Quick examples
------------------

- Use the VM images available from https://bellard.org/jslinux (no
  need to download them):

  Terminal:

  ./temu https://bellard.org/jslinux/buildroot-riscv64.cfg

- Download the example RISC-V Linux image
  (diskimage-linux-riscv-yyyy-mm-dd.tar.gz) and use it:

  ./temu root-riscv64.cfg

- Access to your local hard disk (/tmp directory) in the guest:

  ./temu root_9p-riscv64.cfg

then type:
mount -t 9p /dev/root /mnt

in the guest. The content of the host '/tmp' directory is visible in '/mnt'.

3.2 Invocation
--------------

usage: temu [options] config_file
options are:
-m ram_size       set the RAM size in MB
-rw               allow write access to the disk image (default=snapshot)
-ctrlc            the C-c key stops the emulator instead of being sent to the
                  emulated software
-append cmdline   append cmdline to the kernel command line
Console keys:
Press C-a x to exit the emulator, C-a h to get some help.

3.3 Network usage
-----------------

The easiest way is to use the "user" mode network driver. No specific
configuration is necessary.

TinyEMU also supports a "tap" network driver to redirect the network
traffic from a VirtIO network adapter.

You can look at the netinit.sh script to create the tap network
interface and to redirect the virtual traffic to Internet thru a
NAT. The exact configuration may depend on the Linux distribution and
local firewall configuration.

The VM configuration file must include:

eth0: { driver: "tap", ifname: "tap0" }

and configure the network in the guest system with:

ifconfig eth0 192.168.3.2
route add -net 0.0.0.0 gw 192.168.3.1 eth0

3.4 Network filesystem
----------------------

TinyEMU supports the VirtIO 9P filesystem to access local or remote
filesystems. For remote filesystems, it does HTTP requests to download
the files. The protocol is compatible with the vfsync utility. In the
"mount" command, "/dev/rootN" must be used as device name where N is
the index of the filesystem. When N=0 it is omitted.

The build_filelist tool builds the file list from a root directory. A
simple web server is enough to serve the files.

The '.preload' file gives a list of files to preload when opening a
given file.

3.5 Network block device
------------------------

TinyEMU supports an HTTP block device. The disk image is split into
small files. Use the 'splitimg' utility to generate images. The URL of
the JSON blk.txt file must be provided as disk image filename.

4) Technical notes
------------------

4.1) Floating point emulation

The floating point emulation is bit exact and supports all the
specified instructions for 32 and 64 bit floating point
numbers. It uses the new SoftFP library.

4.3) Consoles

The RISC-V virt machine always exposes its 16550A UART at 0x10000000,
as on QEMU's virt platform. The host terminal is connected to the
VirtIO console by default. Set the following top-level configuration
property to use the UART for both input and output instead:

  console: "uart"

This mode does not create a VirtIO console and is suitable for systems
such as xv6. With the default VirtIO console, UART output can also be
copied to the host terminal for firmware and early kernel messages:

  uart_output: true

Input still goes only to the selected console. If the guest writes to
both devices, output from both appears on the terminal.

4.4) Javascript version

The Javascript version (JSLinux) can be compiled with Makefile.js and
emscripten. A complete precompiled and preconfigured demo is available
in the jslinux-yyyy-mm-dd.tar.gz archive (read the readme.txt file
inside the archive).

5) License / Credits
--------------------

TinyEMU is released under the MIT license. If there is no explicit
license in a file, the license from MIT-LICENSE.txt applies.

The SLIRP library has its own license (two clause BSD license).
