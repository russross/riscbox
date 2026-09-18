//! Flattened device-tree construction for the Riscbox virtual platform.

const MAGIC: u32 = 0xd00d_feed;
const BEGIN_NODE: u32 = 1;
const END_NODE: u32 = 2;
const PROP: u32 = 3;
const END: u32 = 9;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferDescription {
    pub size: u32,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FdtConfig<'a> {
    pub ram_size: u64,
    pub command_line: &'a str,
    pub initrd: Option<(u64, u64)>,
    pub virtio_count: u8,
    pub framebuffer: Option<FramebufferDescription>,
}

#[derive(Default)]
struct Builder {
    structure: Vec<u8>,
    strings: Vec<u8>,
    depth: usize,
}

impl Builder {
    fn word(&mut self, value: u32) {
        self.structure.extend_from_slice(&value.to_be_bytes());
    }

    fn data(&mut self, value: &[u8]) {
        self.structure.extend_from_slice(value);
        while !self.structure.len().is_multiple_of(4) {
            self.structure.push(0);
        }
    }

    fn node(&mut self, name: &str) {
        self.word(BEGIN_NODE);
        let mut bytes = name.as_bytes().to_vec();
        bytes.push(0);
        self.data(&bytes);
        self.depth += 1;
    }

    fn end_node(&mut self) {
        self.word(END_NODE);
        self.depth -= 1;
    }

    fn string_offset(&mut self, name: &str) -> u32 {
        let bytes = name.as_bytes();
        let mut start = 0;
        while start < self.strings.len() {
            let end = self.strings[start..]
                .iter()
                .position(|byte| *byte == 0)
                .map_or(self.strings.len(), |end| start + end);
            if &self.strings[start..end] == bytes {
                return u32::try_from(start).expect("FDT string table offset fits in u32");
            }
            start = end + 1;
        }
        let offset = u32::try_from(self.strings.len()).expect("FDT string table fits in u32");
        self.strings.extend_from_slice(bytes);
        self.strings.push(0);
        offset
    }

    fn property(&mut self, name: &str, value: &[u8]) {
        let offset = self.string_offset(name);
        self.word(PROP);
        self.word(u32::try_from(value.len()).expect("FDT property length fits in u32"));
        self.word(offset);
        self.data(value);
    }

    fn property_u32(&mut self, name: &str, value: u32) {
        self.property(name, &value.to_be_bytes());
    }

    fn property_cells(&mut self, name: &str, cells: &[u32]) {
        let mut bytes = Vec::with_capacity(cells.len() * 4);
        for value in cells {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        self.property(name, &bytes);
    }

    fn property_u64_pair(&mut self, name: &str, first: u64, second: u64) {
        self.property_cells(
            name,
            &[
                high_u32(first),
                low_u32(first),
                high_u32(second),
                low_u32(second),
            ],
        );
    }

    fn property_string(&mut self, name: &str, value: &str) {
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);
        self.property(name, &bytes);
    }

    fn property_strings(&mut self, name: &str, values: &[&str]) {
        let mut bytes = Vec::new();
        for value in values {
            bytes.extend_from_slice(value.as_bytes());
            bytes.push(0);
        }
        self.property(name, &bytes);
    }

    fn finish(mut self) -> Vec<u8> {
        assert_eq!(self.depth, 0);
        self.word(END);
        let structure_offset = 56_u32;
        let string_offset = structure_offset
            + u32::try_from(self.structure.len()).expect("FDT structure fits in u32");
        let total =
            usize::try_from(string_offset).expect("FDT offset fits in usize") + self.strings.len();
        let aligned_total = total.next_multiple_of(8);
        let mut output = Vec::with_capacity(aligned_total);
        for word in [
            MAGIC,
            u32::try_from(aligned_total).expect("FDT size fits in u32"),
            structure_offset,
            string_offset,
            40,
            17,
            16,
            0,
            u32::try_from(self.strings.len()).expect("FDT strings fit in u32"),
            u32::try_from(self.structure.len()).expect("FDT structure fits in u32"),
        ] {
            output.extend_from_slice(&word.to_be_bytes());
        }
        output.extend_from_slice(&[0; 16]);
        output.extend_from_slice(&self.structure);
        output.extend_from_slice(&self.strings);
        output.resize(aligned_total, 0);
        output
    }
}

#[must_use]
pub fn build(config: FdtConfig<'_>) -> Vec<u8> {
    let mut fdt = Builder::default();
    fdt.node("");
    fdt.property_u32("#address-cells", 2);
    fdt.property_u32("#size-cells", 2);
    fdt.property_string("compatible", "riscv-virtio");
    fdt.property_string("model", "riscv-virtio,qemu");

    add_cpu_nodes(&mut fdt);

    fdt.node("memory@80000000");
    fdt.property_string("device_type", "memory");
    fdt.property_u64_pair("reg", 0x8000_0000, config.ram_size);
    fdt.end_node();

    fdt.node("soc");
    fdt.property_u32("#address-cells", 2);
    fdt.property_u32("#size-cells", 2);
    fdt.property_string("compatible", "simple-bus");
    fdt.property("ranges", &[]);
    node(
        &mut fdt,
        "syscon@100000",
        &["sifive,test1", "sifive,test0", "syscon"],
        0x10_0000,
        0x1000,
    );
    fdt.property_u32("phandle", 3);
    fdt.end_node();
    add_interrupt_devices(&mut fdt);
    add_optional_devices(&mut fdt, config.virtio_count, config.framebuffer);
    fdt.end_node();

    fdt.node("poweroff");
    fdt.property_string("compatible", "syscon-poweroff");
    fdt.property_u32("regmap", 3);
    fdt.property_u32("offset", 0);
    fdt.property_u32("value", 0x5555);
    fdt.end_node();
    fdt.node("chosen");
    fdt.property_string("bootargs", config.command_line);
    fdt.property_string("stdout-path", "/soc/serial@10000000");
    if let Some((start, size)) = config.initrd {
        fdt.property_cells("linux,initrd-start", &[high_u32(start), low_u32(start)]);
        let end = start.saturating_add(size);
        fdt.property_cells("linux,initrd-end", &[high_u32(end), low_u32(end)]);
    }
    fdt.end_node();
    fdt.end_node();
    fdt.finish()
}

fn add_cpu_nodes(fdt: &mut Builder) {
    fdt.node("cpus");
    fdt.property_u32("#address-cells", 1);
    fdt.property_u32("#size-cells", 0);
    fdt.property_u32("timebase-frequency", 10_000_000);
    fdt.node("cpu@0");
    fdt.property_string("device_type", "cpu");
    fdt.property_u32("reg", 0);
    fdt.property_string("status", "okay");
    fdt.property_string("compatible", "riscv");
    fdt.property_string("riscv,isa", ISA);
    fdt.property_string("riscv,isa-base", "rv64i");
    fdt.property_strings("riscv,isa-extensions", ISA_EXTENSIONS);
    for property in [
        "riscv,cbom-block-size",
        "riscv,cbop-block-size",
        "riscv,cboz-block-size",
    ] {
        fdt.property_u32(property, 64);
    }
    fdt.property_string("mmu-type", "riscv,sv39");
    fdt.property_u32("clock-frequency", 2_000_000_000);
    fdt.property_u32("phandle", 1);
    fdt.node("interrupt-controller");
    fdt.property_u32("#interrupt-cells", 1);
    fdt.property("interrupt-controller", &[]);
    fdt.property_string("compatible", "riscv,cpu-intc");
    fdt.property_u32("phandle", 2);
    fdt.end_node();
    fdt.end_node();
    fdt.node("cpu-map");
    fdt.node("cluster0");
    fdt.node("core0");
    fdt.property_u32("cpu", 1);
    fdt.end_node();
    fdt.end_node();
    fdt.end_node();
    fdt.end_node();
}

fn add_interrupt_devices(fdt: &mut Builder) {
    fdt.node("clint@2000000");
    fdt.property_strings("compatible", &["sifive,clint0", "riscv,clint0"]);
    fdt.property_cells("interrupts-extended", &[2, 3, 2, 7]);
    fdt.property_u64_pair("reg", 0x200_0000, 0x1_0000);
    fdt.end_node();
    fdt.node("interrupt-controller@c000000");
    fdt.property_u32("#interrupt-cells", 1);
    fdt.property("interrupt-controller", &[]);
    fdt.property_strings("compatible", &["sifive,plic-1.0.0", "riscv,plic0"]);
    fdt.property_u32("riscv,ndev", 31);
    fdt.property_u64_pair("reg", 0xc00_0000, 0x400_0000);
    fdt.property_cells("interrupts-extended", &[2, 11, 2, 9]);
    fdt.property_u32("phandle", 4);
    fdt.end_node();
    interrupt_device(
        fdt,
        "rtc@101000",
        "google,goldfish-rtc",
        0x10_1000,
        0x1000,
        11,
    );
    fdt.node("serial@10000000");
    fdt.property_string("compatible", "ns16550a");
    fdt.property_u64_pair("reg", 0x1000_0000, 0x100);
    fdt.property_cells("interrupts-extended", &[4, 10]);
    fdt.property_u32("clock-frequency", 3_686_400);
    fdt.end_node();
}

fn add_optional_devices(
    fdt: &mut Builder,
    virtio_count: u8,
    framebuffer: Option<FramebufferDescription>,
) {
    for index in 0..virtio_count {
        let address = 0x1000_1000 + u64::from(index) * 0x1000;
        fdt.node(&format!("virtio@{address:x}"));
        fdt.property_string("compatible", "virtio,mmio");
        fdt.property_u64_pair("reg", address, 0x1000);
        fdt.property_cells("interrupts-extended", &[4, u32::from(virtio_irq(index))]);
        fdt.end_node();
    }
    if let Some(framebuffer) = framebuffer {
        fdt.node("framebuffer@4100000");
        fdt.property_string("compatible", "simple-framebuffer");
        fdt.property_u64_pair("reg", 0x410_0000, u64::from(framebuffer.size));
        fdt.property_u32("width", framebuffer.width);
        fdt.property_u32("height", framebuffer.height);
        fdt.property_u32("stride", framebuffer.stride);
        fdt.property_string("format", "a8r8g8b8");
        fdt.end_node();
    }
}

fn node(fdt: &mut Builder, name: &str, compatible: &[&str], address: u64, size: u64) {
    fdt.node(name);
    fdt.property_strings("compatible", compatible);
    fdt.property_u64_pair("reg", address, size);
}

fn interrupt_device(
    fdt: &mut Builder,
    name: &str,
    compatible: &str,
    address: u64,
    size: u64,
    irq: u32,
) {
    fdt.node(name);
    fdt.property_string("compatible", compatible);
    fdt.property_u64_pair("reg", address, size);
    fdt.property_cells("interrupts-extended", &[4, irq]);
    fdt.end_node();
}

const fn virtio_irq(index: u8) -> u8 {
    let irq = 1 + index;
    if irq >= 10 { irq + 2 } else { irq }
}

fn high_u32(value: u64) -> u32 {
    let bytes = value.to_be_bytes();
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn low_u32(value: u64) -> u32 {
    let bytes = value.to_be_bytes();
    u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
}

const ISA: &str = "rv64imafdcb_zicbom_zicbop_zicboz_ziccamoa_ziccif_zicclsm_ziccrse_zicntr_zicond_zicsr_zifencei_zihintntl_zihintpause_zihpm_zimop_za64rs_zawrs_zcb_zcmop_zba_zbb_zbs_ssccptr_sscounterenw_sstc_sstvala_sstvecd_ssu64xl_svadu_svinval_svnapot_svpbmt";
const ISA_EXTENSIONS: &[&str] = &[
    "i",
    "m",
    "a",
    "f",
    "d",
    "c",
    "b",
    "zicbom",
    "zicbop",
    "zicboz",
    "ziccamoa",
    "ziccif",
    "zicclsm",
    "ziccrse",
    "zicntr",
    "zicond",
    "zicsr",
    "zifencei",
    "zihintntl",
    "zihintpause",
    "zihpm",
    "zimop",
    "za64rs",
    "zawrs",
    "zcb",
    "zcmop",
    "zba",
    "zbb",
    "zbs",
    "ssccptr",
    "sscounterenw",
    "sstc",
    "sstvala",
    "sstvecd",
    "ssu64xl",
    "svadu",
    "svinval",
    "svnapot",
    "svpbmt",
];
