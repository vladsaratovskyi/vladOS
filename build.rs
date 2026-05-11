use std::env;
use std::fs;
use std::path::Path;

const PAGE_SIZE: u64 = 4096;
const USER_BASE: u64 = 1 << 39;
const USER_CODE_BASE: u64 = USER_BASE + 0x0040_0000;
const USER_DATA_BASE: u64 = USER_BASE + 0x0060_0000;
const USER_TEST_PAGE_BASE: u64 = USER_BASE + 0x0070_0000;

const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 0x3e;
const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;
const SYS_EXIT: u32 = 1;
const SYS_WRITE: u32 = 2;
const EBADF: u64 = (-9_i64) as u64;
const EFAULT: u64 = (-14_i64) as u64;

struct Segment {
    vaddr: u64,
    flags: u32,
    memsz: u64,
    data: Vec<u8>,
}

fn main() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is not set");
    let out_dir = Path::new(&out_dir);

    write_fixture(out_dir, "exit_42.elf", exit_42());
    write_fixture(out_dir, "write_private_data.elf", write_private_data());
    write_fixture(
        out_dir,
        "write_readonly_segment.elf",
        write_readonly_segment(),
    );
    write_fixture(out_dir, "busy_counter.elf", busy_counter());
    write_fixture(out_dir, "write_syscall_suite.elf", write_syscall_suite());
    write_fixture(out_dir, "write_hello.elf", write_hello());
    write_fixture(out_dir, "read_data_exit.elf", read_data_exit());

    let mut bad_machine = exit_42();
    bad_machine[18..20].copy_from_slice(&3_u16.to_le_bytes());
    write_fixture(out_dir, "bad_machine.elf", bad_machine);
}

fn write_fixture(out_dir: &Path, name: &str, bytes: Vec<u8>) {
    fs::write(out_dir.join(name), bytes).expect("failed to write generated ELF fixture");
}

fn exit_42() -> Vec<u8> {
    let mut code = Vec::new();
    mov_rdi_imm64(&mut code, 42);
    mov_rax_imm32(&mut code, SYS_EXIT);
    int_0x80(&mut code);
    spin(&mut code);

    elf(
        USER_CODE_BASE,
        &[Segment {
            vaddr: USER_CODE_BASE,
            flags: PF_R | PF_X,
            memsz: code.len() as u64,
            data: code,
        }],
    )
}

fn write_private_data() -> Vec<u8> {
    let mut code = Vec::new();
    code.extend_from_slice(&[0x48, 0x89, 0xf8]); // mov rax, rdi
    mov_rbx_imm64(&mut code, USER_DATA_BASE);
    code.extend_from_slice(&[0x48, 0x89, 0x03]); // mov [rbx], rax
    code.extend_from_slice(&[0x48, 0x8b, 0x3b]); // mov rdi, [rbx]
    mov_rax_imm32(&mut code, SYS_EXIT);
    int_0x80(&mut code);
    spin(&mut code);

    elf(
        USER_CODE_BASE,
        &[
            Segment {
                vaddr: USER_CODE_BASE,
                flags: PF_R | PF_X,
                memsz: code.len() as u64,
                data: code,
            },
            Segment {
                vaddr: USER_DATA_BASE,
                flags: PF_R | PF_W,
                memsz: 8,
                data: Vec::new(),
            },
        ],
    )
}

fn write_readonly_segment() -> Vec<u8> {
    let mut code = Vec::new();
    mov_rax_imm64(&mut code, USER_DATA_BASE);
    code.extend_from_slice(&[0x48, 0xc7, 0x00, 0x01, 0x00, 0x00, 0x00]); // mov qword [rax], 1
    mov_rdi_imm64(&mut code, 99);
    mov_rax_imm32(&mut code, SYS_EXIT);
    int_0x80(&mut code);
    spin(&mut code);

    elf(
        USER_CODE_BASE,
        &[
            Segment {
                vaddr: USER_CODE_BASE,
                flags: PF_R | PF_X,
                memsz: code.len() as u64,
                data: code,
            },
            Segment {
                vaddr: USER_DATA_BASE,
                flags: PF_R,
                memsz: 8,
                data: vec![0; 8],
            },
        ],
    )
}

fn write_syscall_suite() -> Vec<u8> {
    let hello = b"hello from user write\n";
    let stderr = b"stderr from user write\n";
    let cross = b"cross-page user write ok\n";
    let stderr_offset = 64;
    let cross_offset = PAGE_SIZE as usize - 5;

    let mut data = vec![0; cross_offset + cross.len()];
    data[..hello.len()].copy_from_slice(hello);
    data[stderr_offset..stderr_offset + stderr.len()].copy_from_slice(stderr);
    data[cross_offset..cross_offset + cross.len()].copy_from_slice(cross);

    let mut code = Vec::new();
    write_syscall(&mut code, 1, USER_DATA_BASE, hello.len() as u64);
    check_rax_eq_continue(&mut code, hello.len() as u64);
    write_syscall(
        &mut code,
        2,
        USER_DATA_BASE + stderr_offset as u64,
        stderr.len() as u64,
    );
    check_rax_eq_continue(&mut code, stderr.len() as u64);
    write_syscall(&mut code, 1, USER_TEST_PAGE_BASE, 4);
    check_rax_eq_continue(&mut code, EFAULT);
    write_syscall(&mut code, 1, 0x20_0000, 4);
    check_rax_eq_continue(&mut code, EFAULT);
    write_syscall(&mut code, 99, USER_DATA_BASE, hello.len() as u64);
    check_rax_eq_continue(&mut code, EBADF);
    write_syscall(
        &mut code,
        1,
        USER_DATA_BASE + cross_offset as u64,
        cross.len() as u64,
    );
    check_rax_eq_continue(&mut code, cross.len() as u64);
    exit_with_code(&mut code, 0);

    elf(
        USER_CODE_BASE,
        &[
            Segment {
                vaddr: USER_CODE_BASE,
                flags: PF_R | PF_X,
                memsz: code.len() as u64,
                data: code,
            },
            Segment {
                vaddr: USER_DATA_BASE,
                flags: PF_R,
                memsz: data.len() as u64,
                data,
            },
        ],
    )
}

fn write_hello() -> Vec<u8> {
    let message = b"preempted write hello\n";
    let mut code = Vec::new();
    write_syscall(&mut code, 1, USER_DATA_BASE, message.len() as u64);
    check_rax_eq_continue(&mut code, message.len() as u64);
    exit_with_code(&mut code, 0);

    elf(
        USER_CODE_BASE,
        &[
            Segment {
                vaddr: USER_CODE_BASE,
                flags: PF_R | PF_X,
                memsz: code.len() as u64,
                data: code,
            },
            Segment {
                vaddr: USER_DATA_BASE,
                flags: PF_R,
                memsz: message.len() as u64,
                data: message.to_vec(),
            },
        ],
    )
}

fn read_data_exit() -> Vec<u8> {
    let mut code = Vec::new();
    mov_rbx_imm64(&mut code, USER_DATA_BASE);
    code.extend_from_slice(&[0x48, 0x8b, 0x3b]); // mov rdi, [rbx]
    mov_rax_imm32(&mut code, SYS_EXIT);
    int_0x80(&mut code);
    spin(&mut code);

    elf(
        USER_CODE_BASE,
        &[
            Segment {
                vaddr: USER_CODE_BASE,
                flags: PF_R | PF_X,
                memsz: code.len() as u64,
                data: code,
            },
            Segment {
                vaddr: USER_DATA_BASE,
                flags: PF_R | PF_W,
                memsz: 8,
                data: Vec::new(),
            },
        ],
    )
}

fn busy_counter() -> Vec<u8> {
    let mut code = Vec::new();
    mov_rbx_imm64(&mut code, USER_DATA_BASE);
    code.extend_from_slice(&[0x48, 0xff, 0x03]); // inc qword [rbx]
    code.extend_from_slice(&[0xeb, 0xfb]); // jmp back to inc

    elf(
        USER_CODE_BASE,
        &[
            Segment {
                vaddr: USER_CODE_BASE,
                flags: PF_R | PF_X,
                memsz: code.len() as u64,
                data: code,
            },
            Segment {
                vaddr: USER_DATA_BASE,
                flags: PF_R | PF_W,
                memsz: 8,
                data: Vec::new(),
            },
        ],
    )
}

fn elf(entry: u64, segments: &[Segment]) -> Vec<u8> {
    let header_size = 64_usize;
    let program_header_size = 56_usize;
    let mut offsets = vec![0_u64; segments.len()];
    let mut file_len = align_up(
        (header_size + program_header_size * segments.len()) as u64,
        PAGE_SIZE,
    );

    for (index, segment) in segments.iter().enumerate() {
        if !segment.data.is_empty() {
            file_len = align_up(file_len, PAGE_SIZE);
            offsets[index] = file_len;
            file_len += segment.data.len() as u64;
        }
    }

    let mut bytes = vec![0_u8; file_len as usize];
    bytes[0..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2; // ELFCLASS64
    bytes[5] = 1; // little endian
    bytes[6] = 1; // current ELF version
    put_u16(&mut bytes, 16, ET_EXEC);
    put_u16(&mut bytes, 18, EM_X86_64);
    put_u32(&mut bytes, 20, 1);
    put_u64(&mut bytes, 24, entry);
    put_u64(&mut bytes, 32, header_size as u64);
    put_u16(&mut bytes, 52, header_size as u16);
    put_u16(&mut bytes, 54, program_header_size as u16);
    put_u16(&mut bytes, 56, segments.len() as u16);

    for (index, segment) in segments.iter().enumerate() {
        let ph = header_size + index * program_header_size;
        put_u32(&mut bytes, ph, PT_LOAD);
        put_u32(&mut bytes, ph + 4, segment.flags);
        put_u64(&mut bytes, ph + 8, offsets[index]);
        put_u64(&mut bytes, ph + 16, segment.vaddr);
        put_u64(&mut bytes, ph + 24, segment.vaddr);
        put_u64(&mut bytes, ph + 32, segment.data.len() as u64);
        put_u64(&mut bytes, ph + 40, segment.memsz);
        put_u64(&mut bytes, ph + 48, PAGE_SIZE);

        if !segment.data.is_empty() {
            let start = offsets[index] as usize;
            bytes[start..start + segment.data.len()].copy_from_slice(&segment.data);
        }
    }

    bytes
}

fn mov_rax_imm64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&[0x48, 0xb8]);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn mov_rbx_imm64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&[0x48, 0xbb]);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn mov_rdi_imm64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&[0x48, 0xbf]);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn mov_rsi_imm64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&[0x48, 0xbe]);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn mov_rdx_imm64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&[0x48, 0xba]);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn mov_rax_imm32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&[0x48, 0xc7, 0xc0]);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_syscall(bytes: &mut Vec<u8>, fd: u64, ptr: u64, len: u64) {
    mov_rax_imm32(bytes, SYS_WRITE);
    mov_rdi_imm64(bytes, fd);
    mov_rsi_imm64(bytes, ptr);
    mov_rdx_imm64(bytes, len);
    int_0x80(bytes);
}

fn check_rax_eq_continue(bytes: &mut Vec<u8>, expected: u64) {
    mov_rbx_imm64(bytes, expected);
    bytes.extend_from_slice(&[0x48, 0x39, 0xd8]); // cmp rax, rbx
    bytes.extend_from_slice(&[0x74, 21]); // je over the failure exit sequence
    exit_with_code(bytes, 1);
}

fn exit_with_code(bytes: &mut Vec<u8>, code: u64) {
    mov_rdi_imm64(bytes, code);
    mov_rax_imm32(bytes, SYS_EXIT);
    int_0x80(bytes);
    spin(bytes);
}

fn int_0x80(bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&[0xcd, 0x80]);
}

fn spin(bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&[0xeb, 0xfe]);
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}
