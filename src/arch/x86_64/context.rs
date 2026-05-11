use core::arch::global_asm;

use x86_64::instructions::segmentation::{Segment, CS, SS};
use x86_64::VirtAddr;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Context {
    rsp: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TrapFrame {
    // Pushed by the low-level interrupt entries in this file.
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    // CPU return state. `rip`, `cs`, and `rflags` are always consumed by
    // `iretq`; `rsp` and `ss` are consumed when returning across privilege
    // levels and are also present in synthetic task-start frames.
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TrapFrameWithErrorCode {
    // Same manually saved register order as `TrapFrame`.
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    // Pushed by the CPU for exceptions such as #GP before the iret frame.
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl Context {
    pub const fn empty() -> Self {
        Self { rsp: 0 }
    }

    pub unsafe fn new_task(stack: &mut [u8], entry_point: usize, rflags: u64) -> Self {
        const STACK_ALIGN: usize = 16;

        let stack_top = stack.as_mut_ptr() as usize + stack.len();
        let stack_top = stack_top & !(STACK_ALIGN - 1);
        let initial_rsp = stack_top - core::mem::size_of::<u64>();
        let frame_bottom = initial_rsp - core::mem::size_of::<TrapFrame>();
        let frame = frame_bottom as *mut TrapFrame;

        // New tasks start through the same interrupt-return restore path used
        // after preemption. `initial_rsp` leaves the trampoline with
        // `rsp % 16 == 8`, matching normal SysV function entry.
        unsafe {
            frame.write(TrapFrame {
                r15: 0,
                r14: 0,
                r13: 0,
                r12: 0,
                r11: 0,
                r10: 0,
                r9: 0,
                r8: 0,
                rsi: 0,
                rdi: 0,
                rbp: 0,
                rdx: 0,
                rcx: 0,
                rbx: 0,
                rax: 0,
                rip: entry_point as u64,
                cs: CS::get_reg().0 as u64,
                rflags: rflags | 0x2,
                rsp: initial_rsp as u64,
                ss: SS::get_reg().0 as u64,
            });
        }

        Self {
            rsp: frame_bottom as u64,
        }
    }

    pub unsafe fn new_user_task(
        stack: &mut [u8],
        entry_point: u64,
        user_stack_top: u64,
        user_code_selector: u16,
        user_data_selector: u16,
        rflags: u64,
        arg0: u64,
    ) -> Self {
        const STACK_ALIGN: usize = 16;

        let kernel_stack_top = stack.as_mut_ptr() as usize + stack.len();
        let kernel_stack_top = kernel_stack_top & !(STACK_ALIGN - 1);
        let frame_bottom = kernel_stack_top - core::mem::size_of::<TrapFrame>();
        let user_rsp = (user_stack_top as usize & !(STACK_ALIGN - 1)) - core::mem::size_of::<u64>();
        let frame = frame_bottom as *mut TrapFrame;

        // User tasks also start through `restore_interrupt_context`: the
        // scheduler selects this frame and `iretq` performs the first CPL3
        // transition using the ring-3 CS/SS and user stack below.
        unsafe {
            frame.write(TrapFrame {
                r15: 0,
                r14: 0,
                r13: 0,
                r12: 0,
                r11: 0,
                r10: 0,
                r9: 0,
                r8: 0,
                rsi: 0,
                rdi: arg0,
                rbp: 0,
                rdx: 0,
                rcx: 0,
                rbx: 0,
                rax: 0,
                rip: entry_point,
                cs: user_code_selector as u64,
                rflags: rflags | 0x2,
                rsp: user_rsp as u64,
                ss: user_data_selector as u64,
            });
        }

        Self {
            rsp: frame_bottom as u64,
        }
    }

    pub fn rsp(&self) -> u64 {
        self.rsp
    }

    pub fn set_rsp(&mut self, rsp: u64) {
        self.rsp = rsp;
    }
}

global_asm!(
    r#"
    .global switch_from_main_to_task
    .type switch_from_main_to_task, @function
switch_from_main_to_task:
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov [rdi], rsp
    mov rsp, [rsi]

    jmp restore_interrupt_context
    .size switch_from_main_to_task, . - switch_from_main_to_task

    .global restore_task_context
    .type restore_task_context, @function
restore_task_context:
    mov rsp, [rdi]
    jmp restore_interrupt_context
    .size restore_task_context, . - restore_task_context

    .global restore_main_context
    .type restore_main_context, @function
restore_main_context:
    mov rsp, [rdi]
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret
    .size restore_main_context, . - restore_main_context

    .global timer_interrupt_entry
    .type timer_interrupt_entry, @function
timer_interrupt_entry:
    // The CPU has already pushed the interrupt return state. From CPL3 it also
    // switched to TSS.rsp0 and pushed the old user rsp/ss. Save all
    // general-purpose registers because timer preemption can happen between any
    // two instructions, not only at a function-call boundary.
    push rax
    push rbx
    push rcx
    push rdx
    push rbp
    push rdi
    push rsi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    cld
    // Pass the trap-frame stack pointer to Rust. The temporary stack
    // alignment is only for the C ABI call; Rust returns the frame pointer
    // that should be resumed.
    mov rdi, rsp
    mov rbp, rsp
    and rsp, -16
    call timer_interrupt_rust
    mov rsp, rax
    jmp restore_interrupt_context
    .size timer_interrupt_entry, . - timer_interrupt_entry

    .global yield_interrupt_entry
    .type yield_interrupt_entry, @function
yield_interrupt_entry:
    // `yield_now()` uses a software interrupt so it shares the exact same
    // full-context save/restore path as timer preemption.
    push rax
    push rbx
    push rcx
    push rdx
    push rbp
    push rdi
    push rsi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    cld
    mov rdi, rsp
    mov rbp, rsp
    and rsp, -16
    call yield_interrupt_rust
    mov rsp, rax
    jmp restore_interrupt_context
    .size yield_interrupt_entry, . - yield_interrupt_entry

    .global syscall_interrupt_entry
    .type syscall_interrupt_entry, @function
syscall_interrupt_entry:
    // Software interrupt syscall entry uses the same saved context shape as
    // timer/yield. From CPL3 the CPU first switches to TSS.rsp0, then pushes
    // ss, rsp, rflags, cs, and rip for `iretq`.
    push rax
    push rbx
    push rcx
    push rdx
    push rbp
    push rdi
    push rsi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    cld
    mov rdi, rsp
    mov rbp, rsp
    and rsp, -16
    call syscall_interrupt_rust
    mov rsp, rax
    jmp restore_interrupt_context
    .size syscall_interrupt_entry, . - syscall_interrupt_entry

    .global general_protection_entry
    .type general_protection_entry, @function
general_protection_entry:
    // #GP includes a CPU-pushed error code before the iret frame. Rust only
    // returns from this stub after choosing a different normal `TrapFrame`;
    // the faulting error-code frame is never restored.
    push rax
    push rbx
    push rcx
    push rdx
    push rbp
    push rdi
    push rsi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    cld
    mov rdi, rsp
    mov rbp, rsp
    and rsp, -16
    call general_protection_rust
    mov rsp, rax
    jmp restore_interrupt_context
    .size general_protection_entry, . - general_protection_entry

    .global page_fault_entry
    .type page_fault_entry, @function
page_fault_entry:
    // #PF has the same error-code stack shape as #GP. Rust may schedule away
    // from a user fault, so use the full trap-frame path instead of a normal
    // `extern "x86-interrupt"` handler.
    push rax
    push rbx
    push rcx
    push rdx
    push rbp
    push rdi
    push rsi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    cld
    mov rdi, rsp
    mov rbp, rsp
    and rsp, -16
    call page_fault_rust
    mov rsp, rax
    jmp restore_interrupt_context
    .size page_fault_entry, . - page_fault_entry

restore_interrupt_context:
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rsi
    pop rdi
    pop rbp
    pop rdx
    pop rcx
    pop rbx
    pop rax
    iretq
"#
);

extern "C" {
    fn switch_from_main_to_task(old_context: *mut Context, new_context: *const Context);
    fn restore_task_context(new_context: *const Context) -> !;
    fn restore_main_context(main_context: *const Context) -> !;
    fn timer_interrupt_entry();
    fn yield_interrupt_entry();
    fn syscall_interrupt_entry();
    fn general_protection_entry();
    fn page_fault_entry();
}

pub unsafe fn switch_from_main(old_context: *mut Context, new_context: *const Context) {
    unsafe {
        switch_from_main_to_task(old_context, new_context);
    }
}

pub unsafe fn restore_task(new_context: *const Context) -> ! {
    unsafe { restore_task_context(new_context) }
}

pub unsafe fn restore_main(main_context: *const Context) -> ! {
    unsafe { restore_main_context(main_context) }
}

pub fn timer_interrupt_entry_addr() -> VirtAddr {
    VirtAddr::new(timer_interrupt_entry as *const () as u64)
}

pub fn yield_interrupt_entry_addr() -> VirtAddr {
    VirtAddr::new(yield_interrupt_entry as *const () as u64)
}

pub fn syscall_interrupt_entry_addr() -> VirtAddr {
    VirtAddr::new(syscall_interrupt_entry as *const () as u64)
}

pub fn general_protection_entry_addr() -> VirtAddr {
    VirtAddr::new(general_protection_entry as *const () as u64)
}

pub fn page_fault_entry_addr() -> VirtAddr {
    VirtAddr::new(page_fault_entry as *const () as u64)
}
