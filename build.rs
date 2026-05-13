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
const SYS_YIELD: u32 = 0;
const SYS_EXIT: u32 = 1;
const SYS_WRITE: u32 = 2;
const SYS_GETPID: u32 = 3;
const SYS_WAITPID: u32 = 4;
const SYS_OPEN: u32 = 5;
const SYS_READ: u32 = 6;
const SYS_CLOSE: u32 = 7;
const SYS_BRK: u32 = 8;
const ENOENT: u64 = (-2_i64) as u64;
const EBADF: u64 = (-9_i64) as u64;
const ECHILD: u64 = (-10_i64) as u64;
const ENOMEM: u64 = (-12_i64) as u64;
const EFAULT: u64 = (-14_i64) as u64;
const EINVAL: u64 = (-22_i64) as u64;
const WNOHANG: u64 = 1;
const O_RDONLY: u64 = 0;
const WAIT_EXITED: u32 = 0;
const WAIT_FAULTED: u32 = 1;

const HELLO_FILE: &[u8] = b"hello from embedded file\n";
const MOTD_FILE: &[u8] = b"tiny kernel says hello\n";
const HELLO_PATH: &[u8] = b"/hello.txt";
const MOTD_PATH: &[u8] = b"/motd";
const MISSING_PATH: &[u8] = b"/missing";

const FD_HELLO_PATH: u64 = USER_DATA_BASE;
const FD_MOTD_PATH: u64 = USER_DATA_BASE + 16;
const FD_MISSING_PATH: u64 = USER_DATA_BASE + 32;
const FD_TEMP_FD_A: u64 = USER_DATA_BASE + 48;
const FD_TEMP_FD_B: u64 = USER_DATA_BASE + 56;
const FD_BUFFER: u64 = USER_DATA_BASE + 128;
const FD_CROSS_BUFFER: u64 = USER_DATA_BASE + PAGE_SIZE - 4;

const HEAP_SCRATCH_BREAK: u64 = USER_DATA_BASE;
const HEAP_SCRATCH_ARG: u64 = USER_DATA_BASE + 8;
const HEAP_MESSAGE: &[u8] = b"hello from brk heap\n";

const PROCESS_PID_DELAYED: u64 = USER_DATA_BASE;
const PROCESS_PID_IMMEDIATE: u64 = USER_DATA_BASE + 8;
const PROCESS_PID_FAULTING: u64 = USER_DATA_BASE + 16;
const PROCESS_PID_NON_CHILD: u64 = USER_DATA_BASE + 24;
const PROCESS_STATUS: u64 = USER_DATA_BASE + 32;

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
    write_fixture(out_dir, "getpid_ok.elf", getpid_ok());
    write_fixture(out_dir, "delayed_exit_child.elf", delayed_exit_child());
    write_fixture(out_dir, "immediate_exit_child.elf", immediate_exit_child());
    write_fixture(out_dir, "faulting_child.elf", faulting_child());
    write_fixture(
        out_dir,
        "process_wait_parent_suite.elf",
        process_wait_parent_suite(),
    );
    write_fixture(out_dir, "fd_syscall_suite.elf", fd_syscall_suite());
    write_fixture(out_dir, "fd_first_open_exit.elf", fd_first_open_exit());
    write_fixture(out_dir, "fd_open_leak_exit.elf", fd_open_leak_exit());
    write_fixture(
        out_dir,
        "brk_query_invalid_suite.elf",
        brk_query_invalid_suite(),
    );
    write_fixture(out_dir, "brk_growth_suite.elf", brk_growth_suite());
    write_fixture(out_dir, "brk_shrink_fault.elf", brk_shrink_fault());
    write_fixture(out_dir, "brk_shrink_continue.elf", brk_shrink_continue());
    write_fixture(out_dir, "brk_private_writer.elf", brk_private_writer());
    write_fixture(out_dir, "brk_busy_counter.elf", brk_busy_counter());

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

fn getpid_ok() -> Vec<u8> {
    let mut code = Vec::new();
    mov_rax_imm32(&mut code, SYS_GETPID);
    int_0x80(&mut code);
    check_rax_nonzero_continue(&mut code);
    exit_with_code(&mut code, 0);

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

fn delayed_exit_child() -> Vec<u8> {
    let mut code = Vec::new();
    yield_syscall(&mut code);
    yield_syscall(&mut code);
    yield_syscall(&mut code);
    exit_with_code(&mut code, 42);

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

fn immediate_exit_child() -> Vec<u8> {
    let mut code = Vec::new();
    exit_with_code(&mut code, 7);

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

fn faulting_child() -> Vec<u8> {
    let mut code = Vec::new();
    mov_rax_imm64(&mut code, USER_TEST_PAGE_BASE);
    code.extend_from_slice(&[0x48, 0xc7, 0x00, 0x01, 0x00, 0x00, 0x00]); // mov qword [rax], 1
    exit_with_code(&mut code, 99);

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

fn process_wait_parent_suite() -> Vec<u8> {
    let mut code = Vec::new();

    mov_rax_imm32(&mut code, SYS_GETPID);
    int_0x80(&mut code);
    check_rax_nonzero_continue(&mut code);

    yield_syscall(&mut code);

    waitpid_from_mem(&mut code, PROCESS_PID_NON_CHILD, PROCESS_STATUS, 0);
    check_rax_eq_continue(&mut code, ECHILD);

    waitpid_from_mem(&mut code, PROCESS_PID_IMMEDIATE, USER_TEST_PAGE_BASE, 0);
    check_rax_eq_continue(&mut code, EFAULT);

    waitpid_from_mem(&mut code, PROCESS_PID_IMMEDIATE, PROCESS_STATUS, 0);
    check_rax_eq_mem_continue(&mut code, PROCESS_PID_IMMEDIATE);
    check_u32_mem_eq_continue(&mut code, PROCESS_STATUS, WAIT_EXITED);
    check_i32_mem_eq_continue(&mut code, PROCESS_STATUS + 4, 7);

    waitpid_from_mem(&mut code, PROCESS_PID_DELAYED, PROCESS_STATUS, WNOHANG);
    check_rax_eq_continue(&mut code, 0);

    waitpid_from_mem(&mut code, PROCESS_PID_DELAYED, USER_TEST_PAGE_BASE, 0);
    check_rax_eq_continue(&mut code, EFAULT);

    waitpid_from_mem(&mut code, PROCESS_PID_DELAYED, PROCESS_STATUS, 0);
    check_rax_eq_mem_continue(&mut code, PROCESS_PID_DELAYED);
    check_u32_mem_eq_continue(&mut code, PROCESS_STATUS, WAIT_EXITED);
    check_i32_mem_eq_continue(&mut code, PROCESS_STATUS + 4, 42);

    waitpid_from_mem(&mut code, PROCESS_PID_DELAYED, PROCESS_STATUS, 0);
    check_rax_eq_continue(&mut code, ECHILD);

    waitpid_from_mem(&mut code, PROCESS_PID_FAULTING, PROCESS_STATUS, 0);
    check_rax_eq_mem_continue(&mut code, PROCESS_PID_FAULTING);
    check_u32_mem_eq_continue(&mut code, PROCESS_STATUS, WAIT_FAULTED);

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
                flags: PF_R | PF_W,
                memsz: 4096,
                data: vec![0; 64],
            },
        ],
    )
}

fn fd_syscall_suite() -> Vec<u8> {
    let mut code = Vec::new();

    read_syscall_fd_imm(&mut code, 0, FD_BUFFER, 8);
    check_rax_eq_continue(&mut code, 0);

    open_syscall(&mut code, FD_HELLO_PATH, HELLO_PATH.len() as u64, O_RDONLY);
    check_rax_eq_continue(&mut code, 3);
    store_rax(&mut code, FD_TEMP_FD_A);
    read_syscall_fd_mem(&mut code, FD_TEMP_FD_A, FD_BUFFER, HELLO_FILE.len() as u64);
    check_rax_eq_continue(&mut code, HELLO_FILE.len() as u64);
    check_byte_mem_eq_continue(&mut code, FD_BUFFER, b'h');
    write_syscall_fd_imm(&mut code, 1, FD_BUFFER, HELLO_FILE.len() as u64);
    check_rax_eq_continue(&mut code, HELLO_FILE.len() as u64);
    read_syscall_fd_mem(&mut code, FD_TEMP_FD_A, FD_BUFFER, 1);
    check_rax_eq_continue(&mut code, 0);
    close_syscall_fd_mem(&mut code, FD_TEMP_FD_A);
    check_rax_eq_continue(&mut code, 0);
    close_syscall_fd_mem(&mut code, FD_TEMP_FD_A);
    check_rax_eq_continue(&mut code, EBADF);
    read_syscall_fd_mem(&mut code, FD_TEMP_FD_A, FD_BUFFER, 1);
    check_rax_eq_continue(&mut code, EBADF);

    open_syscall(
        &mut code,
        FD_MISSING_PATH,
        MISSING_PATH.len() as u64,
        O_RDONLY,
    );
    check_rax_eq_continue(&mut code, ENOENT);
    open_syscall(&mut code, USER_TEST_PAGE_BASE, 4, O_RDONLY);
    check_rax_eq_continue(&mut code, EFAULT);
    open_syscall(&mut code, FD_HELLO_PATH, HELLO_PATH.len() as u64, 99);
    check_rax_eq_continue(&mut code, EINVAL);

    open_syscall(&mut code, FD_HELLO_PATH, HELLO_PATH.len() as u64, O_RDONLY);
    check_rax_eq_continue(&mut code, 3);
    store_rax(&mut code, FD_TEMP_FD_A);
    close_syscall_fd_mem(&mut code, FD_TEMP_FD_A);
    check_rax_eq_continue(&mut code, 0);
    open_syscall(&mut code, FD_MOTD_PATH, MOTD_PATH.len() as u64, O_RDONLY);
    check_rax_eq_continue(&mut code, 3);
    store_rax(&mut code, FD_TEMP_FD_A);
    read_syscall_fd_mem(&mut code, FD_TEMP_FD_A, FD_BUFFER, MOTD_FILE.len() as u64);
    check_rax_eq_continue(&mut code, MOTD_FILE.len() as u64);
    write_syscall_fd_imm(&mut code, 1, FD_BUFFER, MOTD_FILE.len() as u64);
    check_rax_eq_continue(&mut code, MOTD_FILE.len() as u64);
    close_syscall_fd_mem(&mut code, FD_TEMP_FD_A);
    check_rax_eq_continue(&mut code, 0);

    open_syscall(&mut code, FD_HELLO_PATH, HELLO_PATH.len() as u64, O_RDONLY);
    check_rax_eq_continue(&mut code, 3);
    store_rax(&mut code, FD_TEMP_FD_A);
    read_syscall_fd_mem(&mut code, FD_TEMP_FD_A, USER_TEST_PAGE_BASE, 4);
    check_rax_eq_continue(&mut code, EFAULT);
    read_syscall_fd_mem(&mut code, FD_TEMP_FD_A, FD_BUFFER, 1);
    check_rax_eq_continue(&mut code, 1);
    check_byte_mem_eq_continue(&mut code, FD_BUFFER, b'h');
    close_syscall_fd_mem(&mut code, FD_TEMP_FD_A);
    check_rax_eq_continue(&mut code, 0);

    open_syscall(&mut code, FD_HELLO_PATH, HELLO_PATH.len() as u64, O_RDONLY);
    check_rax_eq_continue(&mut code, 3);
    store_rax(&mut code, FD_TEMP_FD_A);
    open_syscall(&mut code, FD_HELLO_PATH, HELLO_PATH.len() as u64, O_RDONLY);
    check_rax_eq_continue(&mut code, 4);
    store_rax(&mut code, FD_TEMP_FD_B);
    read_syscall_fd_mem(&mut code, FD_TEMP_FD_A, FD_BUFFER, 1);
    check_rax_eq_continue(&mut code, 1);
    read_syscall_fd_mem(&mut code, FD_TEMP_FD_B, FD_BUFFER + 1, 1);
    check_rax_eq_continue(&mut code, 1);
    check_byte_mem_eq_continue(&mut code, FD_BUFFER, b'h');
    check_byte_mem_eq_continue(&mut code, FD_BUFFER + 1, b'h');
    close_syscall_fd_mem(&mut code, FD_TEMP_FD_A);
    check_rax_eq_continue(&mut code, 0);
    close_syscall_fd_mem(&mut code, FD_TEMP_FD_B);
    check_rax_eq_continue(&mut code, 0);

    open_syscall(&mut code, FD_HELLO_PATH, HELLO_PATH.len() as u64, O_RDONLY);
    check_rax_eq_continue(&mut code, 3);
    store_rax(&mut code, FD_TEMP_FD_A);
    write_syscall_fd_mem(&mut code, FD_TEMP_FD_A, FD_BUFFER, 1);
    check_rax_eq_continue(&mut code, EBADF);
    close_syscall_fd_mem(&mut code, FD_TEMP_FD_A);
    check_rax_eq_continue(&mut code, 0);

    read_syscall_fd_imm(&mut code, 1, FD_BUFFER, 1);
    check_rax_eq_continue(&mut code, EBADF);

    open_syscall(&mut code, FD_HELLO_PATH, HELLO_PATH.len() as u64, O_RDONLY);
    check_rax_eq_continue(&mut code, 3);
    store_rax(&mut code, FD_TEMP_FD_A);
    read_syscall_fd_mem(
        &mut code,
        FD_TEMP_FD_A,
        FD_CROSS_BUFFER,
        HELLO_FILE.len() as u64,
    );
    check_rax_eq_continue(&mut code, HELLO_FILE.len() as u64);
    check_byte_mem_eq_continue(&mut code, FD_CROSS_BUFFER, b'h');
    check_byte_mem_eq_continue(&mut code, FD_CROSS_BUFFER + 4, b'o');
    close_syscall_fd_mem(&mut code, FD_TEMP_FD_A);
    check_rax_eq_continue(&mut code, 0);

    exit_with_code(&mut code, 0);

    fd_elf(code)
}

fn fd_first_open_exit() -> Vec<u8> {
    let mut code = Vec::new();
    open_syscall(&mut code, FD_HELLO_PATH, HELLO_PATH.len() as u64, O_RDONLY);
    check_rax_eq_continue(&mut code, 3);
    exit_with_code(&mut code, 0);

    fd_elf(code)
}

fn fd_open_leak_exit() -> Vec<u8> {
    let mut code = Vec::new();
    open_syscall(&mut code, FD_HELLO_PATH, HELLO_PATH.len() as u64, O_RDONLY);
    check_rax_eq_continue(&mut code, 3);
    exit_with_code(&mut code, 0);

    fd_elf(code)
}

fn fd_elf(code: Vec<u8>) -> Vec<u8> {
    let mut data = vec![0; PAGE_SIZE as usize * 2];
    data[0..HELLO_PATH.len()].copy_from_slice(HELLO_PATH);
    data[16..16 + MOTD_PATH.len()].copy_from_slice(MOTD_PATH);
    data[32..32 + MISSING_PATH.len()].copy_from_slice(MISSING_PATH);

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
                memsz: data.len() as u64,
                data,
            },
        ],
    )
}

fn brk_query_invalid_suite() -> Vec<u8> {
    let mut code = Vec::new();

    brk_query(&mut code);
    check_rax_nonzero_continue(&mut code);
    store_rax(&mut code, HEAP_SCRATCH_BREAK);

    brk_from_rax_delta(&mut code, -1);
    check_rax_eq_continue(&mut code, EINVAL);
    brk_query(&mut code);
    check_rax_eq_mem_continue(&mut code, HEAP_SCRATCH_BREAK);

    brk_syscall_imm(&mut code, USER_TEST_PAGE_BASE + PAGE_SIZE);
    check_rax_eq_continue(&mut code, ENOMEM);
    brk_query(&mut code);
    check_rax_eq_mem_continue(&mut code, HEAP_SCRATCH_BREAK);

    exit_with_code(&mut code, 0);

    heap_elf(code)
}

fn brk_growth_suite() -> Vec<u8> {
    let mut code = Vec::new();

    brk_query(&mut code);
    store_rax(&mut code, HEAP_SCRATCH_BREAK);
    brk_from_rax_delta(&mut code, PAGE_SIZE as i32);
    check_rax_eq_mem_plus_continue(&mut code, HEAP_SCRATCH_BREAK, PAGE_SIZE as i32);
    check_byte_ptr_mem_eq_continue(&mut code, HEAP_SCRATCH_BREAK, 0, 0);
    check_byte_ptr_mem_eq_continue(&mut code, HEAP_SCRATCH_BREAK, 128, 0);
    check_byte_ptr_mem_eq_continue(&mut code, HEAP_SCRATCH_BREAK, PAGE_SIZE as i32 - 1, 0);

    write_byte_ptr_mem(&mut code, HEAP_SCRATCH_BREAK, 0, 0x5a);
    check_byte_ptr_mem_eq_continue(&mut code, HEAP_SCRATCH_BREAK, 0, 0x5a);

    brk_query(&mut code);
    brk_from_rax_delta(&mut code, (2 * PAGE_SIZE) as i32);
    check_rax_eq_mem_plus_continue(&mut code, HEAP_SCRATCH_BREAK, (3 * PAGE_SIZE) as i32);

    write_byte_ptr_mem(&mut code, HEAP_SCRATCH_BREAK, PAGE_SIZE as i32 - 1, 0xa1);
    write_byte_ptr_mem(&mut code, HEAP_SCRATCH_BREAK, PAGE_SIZE as i32, 0xb2);
    write_byte_ptr_mem(&mut code, HEAP_SCRATCH_BREAK, (2 * PAGE_SIZE) as i32, 0xc3);
    check_byte_ptr_mem_eq_continue(&mut code, HEAP_SCRATCH_BREAK, PAGE_SIZE as i32 - 1, 0xa1);
    check_byte_ptr_mem_eq_continue(&mut code, HEAP_SCRATCH_BREAK, PAGE_SIZE as i32, 0xb2);
    check_byte_ptr_mem_eq_continue(&mut code, HEAP_SCRATCH_BREAK, (2 * PAGE_SIZE) as i32, 0xc3);

    write_bytes_ptr_mem(&mut code, HEAP_SCRATCH_BREAK, 64, HEAP_MESSAGE);
    write_syscall_fd_ptr_mem(
        &mut code,
        1,
        HEAP_SCRATCH_BREAK,
        64,
        HEAP_MESSAGE.len() as u64,
    );
    check_rax_eq_continue(&mut code, HEAP_MESSAGE.len() as u64);

    brk_query(&mut code);
    brk_from_rax_delta(&mut code, 1);
    check_rax_eq_mem_plus_continue(&mut code, HEAP_SCRATCH_BREAK, (3 * PAGE_SIZE) as i32 + 1);

    exit_with_code(&mut code, 0);

    heap_elf(code)
}

fn brk_shrink_fault() -> Vec<u8> {
    let mut code = Vec::new();

    brk_query(&mut code);
    store_rax(&mut code, HEAP_SCRATCH_BREAK);
    brk_from_rax_delta(&mut code, (2 * PAGE_SIZE) as i32);
    check_rax_eq_mem_plus_continue(&mut code, HEAP_SCRATCH_BREAK, (2 * PAGE_SIZE) as i32);

    brk_from_mem_delta(&mut code, HEAP_SCRATCH_BREAK, PAGE_SIZE as i32);
    check_rax_eq_mem_plus_continue(&mut code, HEAP_SCRATCH_BREAK, PAGE_SIZE as i32);

    load_rbx_from_mem_plus(&mut code, HEAP_SCRATCH_BREAK, PAGE_SIZE as i32);
    code.extend_from_slice(&[0x48, 0x8b, 0x03]); // mov rax, [rbx]
    exit_with_code(&mut code, 99);

    heap_elf(code)
}

fn brk_shrink_continue() -> Vec<u8> {
    let mut code = Vec::new();

    brk_query(&mut code);
    store_rax(&mut code, HEAP_SCRATCH_BREAK);
    brk_from_rax_delta(&mut code, (2 * PAGE_SIZE) as i32);
    check_rax_eq_mem_plus_continue(&mut code, HEAP_SCRATCH_BREAK, (2 * PAGE_SIZE) as i32);

    write_byte_ptr_mem(&mut code, HEAP_SCRATCH_BREAK, 0, 0x77);
    write_byte_ptr_mem(&mut code, HEAP_SCRATCH_BREAK, PAGE_SIZE as i32, 0x66);

    brk_from_mem_delta(&mut code, HEAP_SCRATCH_BREAK, 1);
    check_rax_eq_mem_plus_continue(&mut code, HEAP_SCRATCH_BREAK, 1);
    check_byte_ptr_mem_eq_continue(&mut code, HEAP_SCRATCH_BREAK, 0, 0x77);

    exit_with_code(&mut code, 0);

    heap_elf(code)
}

fn brk_private_writer() -> Vec<u8> {
    let mut code = Vec::new();

    store_rdi(&mut code, HEAP_SCRATCH_ARG);
    brk_query(&mut code);
    store_rax(&mut code, HEAP_SCRATCH_BREAK);
    brk_from_rax_delta(&mut code, PAGE_SIZE as i32);
    check_rax_eq_mem_plus_continue(&mut code, HEAP_SCRATCH_BREAK, PAGE_SIZE as i32);

    mov_rbx_imm64(&mut code, HEAP_SCRATCH_ARG);
    code.extend_from_slice(&[0x48, 0x8b, 0x3b]); // mov rdi, [rbx]
    load_rbx_from_mem_plus(&mut code, HEAP_SCRATCH_BREAK, 0);
    code.extend_from_slice(&[0x48, 0x89, 0x3b]); // mov [rbx], rdi

    exit_with_code(&mut code, 0);

    heap_elf(code)
}

fn brk_busy_counter() -> Vec<u8> {
    let mut code = Vec::new();

    brk_query(&mut code);
    store_rax(&mut code, HEAP_SCRATCH_BREAK);
    brk_from_rax_delta(&mut code, PAGE_SIZE as i32);
    check_rax_eq_mem_plus_continue(&mut code, HEAP_SCRATCH_BREAK, PAGE_SIZE as i32);
    load_rbx_from_mem_plus(&mut code, HEAP_SCRATCH_BREAK, 0);
    code.extend_from_slice(&[0x48, 0xff, 0x03]); // inc qword [rbx]
    code.extend_from_slice(&[0xeb, 0xfb]); // jmp back to inc

    heap_elf(code)
}

fn heap_elf(code: Vec<u8>) -> Vec<u8> {
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
                memsz: PAGE_SIZE,
                data: vec![0; PAGE_SIZE as usize],
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

fn open_syscall(bytes: &mut Vec<u8>, path: u64, len: u64, flags: u64) {
    mov_rax_imm32(bytes, SYS_OPEN);
    mov_rdi_imm64(bytes, path);
    mov_rsi_imm64(bytes, len);
    mov_rdx_imm64(bytes, flags);
    int_0x80(bytes);
}

fn read_syscall_fd_imm(bytes: &mut Vec<u8>, fd: u64, buffer: u64, len: u64) {
    mov_rax_imm32(bytes, SYS_READ);
    mov_rdi_imm64(bytes, fd);
    mov_rsi_imm64(bytes, buffer);
    mov_rdx_imm64(bytes, len);
    int_0x80(bytes);
}

fn read_syscall_fd_mem(bytes: &mut Vec<u8>, fd_addr: u64, buffer: u64, len: u64) {
    mov_rax_imm32(bytes, SYS_READ);
    mov_rbx_imm64(bytes, fd_addr);
    bytes.extend_from_slice(&[0x48, 0x8b, 0x3b]); // mov rdi, [rbx]
    mov_rsi_imm64(bytes, buffer);
    mov_rdx_imm64(bytes, len);
    int_0x80(bytes);
}

fn write_syscall_fd_imm(bytes: &mut Vec<u8>, fd: u64, buffer: u64, len: u64) {
    mov_rax_imm32(bytes, SYS_WRITE);
    mov_rdi_imm64(bytes, fd);
    mov_rsi_imm64(bytes, buffer);
    mov_rdx_imm64(bytes, len);
    int_0x80(bytes);
}

fn write_syscall_fd_mem(bytes: &mut Vec<u8>, fd_addr: u64, buffer: u64, len: u64) {
    mov_rax_imm32(bytes, SYS_WRITE);
    mov_rbx_imm64(bytes, fd_addr);
    bytes.extend_from_slice(&[0x48, 0x8b, 0x3b]); // mov rdi, [rbx]
    mov_rsi_imm64(bytes, buffer);
    mov_rdx_imm64(bytes, len);
    int_0x80(bytes);
}

fn close_syscall_fd_mem(bytes: &mut Vec<u8>, fd_addr: u64) {
    mov_rax_imm32(bytes, SYS_CLOSE);
    mov_rbx_imm64(bytes, fd_addr);
    bytes.extend_from_slice(&[0x48, 0x8b, 0x3b]); // mov rdi, [rbx]
    int_0x80(bytes);
}

fn brk_query(bytes: &mut Vec<u8>) {
    brk_syscall_imm(bytes, 0);
}

fn brk_syscall_imm(bytes: &mut Vec<u8>, requested: u64) {
    mov_rax_imm32(bytes, SYS_BRK);
    mov_rdi_imm64(bytes, requested);
    int_0x80(bytes);
}

fn brk_from_rax_delta(bytes: &mut Vec<u8>, delta: i32) {
    bytes.extend_from_slice(&[0x48, 0x89, 0xc7]); // mov rdi, rax
    add_rdi_i32(bytes, delta);
    mov_rax_imm32(bytes, SYS_BRK);
    int_0x80(bytes);
}

fn brk_from_mem_delta(bytes: &mut Vec<u8>, address: u64, delta: i32) {
    mov_rbx_imm64(bytes, address);
    bytes.extend_from_slice(&[0x48, 0x8b, 0x3b]); // mov rdi, [rbx]
    add_rdi_i32(bytes, delta);
    mov_rax_imm32(bytes, SYS_BRK);
    int_0x80(bytes);
}

fn write_syscall_fd_ptr_mem(bytes: &mut Vec<u8>, fd: u64, ptr_addr: u64, offset: i32, len: u64) {
    mov_rax_imm32(bytes, SYS_WRITE);
    mov_rdi_imm64(bytes, fd);
    mov_rbx_imm64(bytes, ptr_addr);
    bytes.extend_from_slice(&[0x48, 0x8b, 0x33]); // mov rsi, [rbx]
    add_rsi_i32(bytes, offset);
    mov_rdx_imm64(bytes, len);
    int_0x80(bytes);
}

fn load_rbx_from_mem_plus(bytes: &mut Vec<u8>, address: u64, offset: i32) {
    mov_rbx_imm64(bytes, address);
    bytes.extend_from_slice(&[0x48, 0x8b, 0x1b]); // mov rbx, [rbx]
    add_rbx_i32(bytes, offset);
}

fn write_byte_ptr_mem(bytes: &mut Vec<u8>, ptr_addr: u64, offset: i32, value: u8) {
    load_rbx_from_mem_plus(bytes, ptr_addr, offset);
    bytes.extend_from_slice(&[0xc6, 0x03, value]); // mov byte [rbx], imm8
}

fn write_bytes_ptr_mem(bytes: &mut Vec<u8>, ptr_addr: u64, offset: i32, values: &[u8]) {
    for (index, value) in values.iter().enumerate() {
        write_byte_ptr_mem(bytes, ptr_addr, offset + index as i32, *value);
    }
}

fn check_byte_ptr_mem_eq_continue(bytes: &mut Vec<u8>, ptr_addr: u64, offset: i32, expected: u8) {
    load_rbx_from_mem_plus(bytes, ptr_addr, offset);
    bytes.extend_from_slice(&[0x80, 0x3b, expected]); // cmp byte [rbx], imm8
    bytes.extend_from_slice(&[0x74, 21]); // je over the failure exit sequence
    exit_with_code(bytes, 1);
}

fn yield_syscall(bytes: &mut Vec<u8>) {
    mov_rax_imm32(bytes, SYS_YIELD);
    int_0x80(bytes);
}

fn waitpid_from_mem(bytes: &mut Vec<u8>, pid_addr: u64, status_addr: u64, options: u64) {
    mov_rax_imm32(bytes, SYS_WAITPID);
    mov_rbx_imm64(bytes, pid_addr);
    bytes.extend_from_slice(&[0x48, 0x8b, 0x3b]); // mov rdi, [rbx]
    mov_rsi_imm64(bytes, status_addr);
    mov_rdx_imm64(bytes, options);
    int_0x80(bytes);
}

fn check_rax_eq_continue(bytes: &mut Vec<u8>, expected: u64) {
    mov_rbx_imm64(bytes, expected);
    bytes.extend_from_slice(&[0x48, 0x39, 0xd8]); // cmp rax, rbx
    bytes.extend_from_slice(&[0x74, 21]); // je over the failure exit sequence
    exit_with_code(bytes, 1);
}

fn check_rax_eq_mem_continue(bytes: &mut Vec<u8>, address: u64) {
    mov_rbx_imm64(bytes, address);
    bytes.extend_from_slice(&[0x48, 0x3b, 0x03]); // cmp rax, [rbx]
    bytes.extend_from_slice(&[0x74, 21]); // je over the failure exit sequence
    exit_with_code(bytes, 1);
}

fn check_rax_eq_mem_plus_continue(bytes: &mut Vec<u8>, address: u64, delta: i32) {
    mov_rbx_imm64(bytes, address);
    bytes.extend_from_slice(&[0x48, 0x8b, 0x0b]); // mov rcx, [rbx]
    add_rcx_i32(bytes, delta);
    bytes.extend_from_slice(&[0x48, 0x39, 0xc8]); // cmp rax, rcx
    bytes.extend_from_slice(&[0x74, 21]); // je over the failure exit sequence
    exit_with_code(bytes, 1);
}

fn check_rax_nonzero_continue(bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&[0x48, 0x85, 0xc0]); // test rax, rax
    bytes.extend_from_slice(&[0x75, 21]); // jne over the failure exit sequence
    exit_with_code(bytes, 1);
}

fn check_u32_mem_eq_continue(bytes: &mut Vec<u8>, address: u64, expected: u32) {
    mov_rbx_imm64(bytes, address);
    bytes.extend_from_slice(&[0x81, 0x3b]); // cmp dword [rbx], imm32
    bytes.extend_from_slice(&expected.to_le_bytes());
    bytes.extend_from_slice(&[0x74, 21]); // je over the failure exit sequence
    exit_with_code(bytes, 1);
}

fn check_i32_mem_eq_continue(bytes: &mut Vec<u8>, address: u64, expected: i32) {
    check_u32_mem_eq_continue(bytes, address, expected as u32);
}

fn check_byte_mem_eq_continue(bytes: &mut Vec<u8>, address: u64, expected: u8) {
    mov_rbx_imm64(bytes, address);
    bytes.extend_from_slice(&[0x80, 0x3b, expected]); // cmp byte [rbx], imm8
    bytes.extend_from_slice(&[0x74, 21]); // je over the failure exit sequence
    exit_with_code(bytes, 1);
}

fn store_rax(bytes: &mut Vec<u8>, address: u64) {
    mov_rbx_imm64(bytes, address);
    bytes.extend_from_slice(&[0x48, 0x89, 0x03]); // mov [rbx], rax
}

fn store_rdi(bytes: &mut Vec<u8>, address: u64) {
    mov_rbx_imm64(bytes, address);
    bytes.extend_from_slice(&[0x48, 0x89, 0x3b]); // mov [rbx], rdi
}

fn add_rbx_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&[0x48, 0x81, 0xc3]);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn add_rcx_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&[0x48, 0x81, 0xc1]);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn add_rdi_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&[0x48, 0x81, 0xc7]);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn add_rsi_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&[0x48, 0x81, 0xc6]);
    bytes.extend_from_slice(&value.to_le_bytes());
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
