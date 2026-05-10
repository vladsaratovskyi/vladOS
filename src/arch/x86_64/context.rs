use core::arch::global_asm;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Context {
    rsp: u64,
}

impl Context {
    pub const fn empty() -> Self {
        Self { rsp: 0 }
    }

    pub unsafe fn new_task(stack: &mut [u8], entry_point: usize) -> Self {
        const STACK_ALIGN: usize = 16;
        const INITIAL_FRAME_SIZE: usize = 8 * core::mem::size_of::<u64>();

        let stack_top = stack.as_mut_ptr() as usize + stack.len();
        let stack_top = stack_top & !(STACK_ALIGN - 1);
        let frame_bottom = stack_top - INITIAL_FRAME_SIZE;
        let frame = frame_bottom as *mut u64;

        // The switch routine restores r15, r14, r13, r12, rbx, and rbp, then
        // returns into entry_point. The last slot is padding so entry starts
        // with the SysV ABI stack alignment: rsp % 16 == 8.
        unsafe {
            frame.add(0).write(0);
            frame.add(1).write(0);
            frame.add(2).write(0);
            frame.add(3).write(0);
            frame.add(4).write(0);
            frame.add(5).write(0);
            frame.add(6).write(entry_point as u64);
            frame.add(7).write(0);
        }

        Self {
            rsp: frame_bottom as u64,
        }
    }
}

global_asm!(
    r#"
    .global context_switch
    .type context_switch, @function
context_switch:
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov [rdi], rsp
    mov rsp, [rsi]

    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret
    .size context_switch, . - context_switch
"#
);

extern "C" {
    fn context_switch(old_context: *mut Context, new_context: *const Context);
}

pub unsafe fn switch(old_context: *mut Context, new_context: *const Context) {
    unsafe {
        context_switch(old_context, new_context);
    }
}
