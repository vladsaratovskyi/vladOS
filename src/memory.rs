use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use spin::Mutex;
use x86_64::{
    registers::control::Cr3,
    structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB},
    PhysAddr, VirtAddr,
};

static MEMORY_STATE: Mutex<Option<MemoryState>> = Mutex::new(None);

pub(crate) struct MemoryState {
    physical_memory_offset: VirtAddr,
    kernel_level_4_frame: PhysFrame<Size4KiB>,
    frame_allocator: BootInfoFrameAllocator,
}

pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = unsafe { active_level_4_table(physical_memory_offset) };

    unsafe { OffsetPageTable::new(level_4_table, physical_memory_offset) }
}

pub fn init_global(physical_memory_offset: VirtAddr, frame_allocator: BootInfoFrameAllocator) {
    let (kernel_level_4_frame, _) = Cr3::read();

    *MEMORY_STATE.lock() = Some(MemoryState {
        physical_memory_offset,
        kernel_level_4_frame,
        frame_allocator,
    });
}

pub fn kernel_level_4_frame() -> PhysFrame<Size4KiB> {
    if let Some(state) = MEMORY_STATE.lock().as_ref() {
        state.kernel_level_4_frame
    } else {
        Cr3::read().0
    }
}

pub(crate) fn with_state<R>(f: impl FnOnce(&mut MemoryState) -> R) -> R {
    let mut guard = MEMORY_STATE.lock();
    let state = guard
        .as_mut()
        .expect("global memory state was not initialized");

    f(state)
}

unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
}

pub struct BootInfoFrameAllocator {
    memory_map: &'static MemoryMap,
    next: usize,
}

impl BootInfoFrameAllocator {
    pub unsafe fn init(memory_map: &'static MemoryMap) -> Self {
        Self {
            memory_map,
            next: 0,
        }
    }

    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        let regions = self.memory_map.iter();
        let usable_regions =
            regions.filter(|region| region.region_type == MemoryRegionType::Usable);
        let addr_ranges =
            usable_regions.map(|region| region.range.start_addr()..region.range.end_addr());
        let frame_addresses = addr_ranges.flat_map(|range| range.step_by(4096));

        frame_addresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}

impl MemoryState {
    pub(crate) fn physical_memory_offset(&self) -> VirtAddr {
        self.physical_memory_offset
    }

    pub(crate) fn kernel_level_4_frame(&self) -> PhysFrame<Size4KiB> {
        self.kernel_level_4_frame
    }

    pub(crate) fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        self.frame_allocator.allocate_frame()
    }

    pub(crate) fn frame_allocator_mut(&mut self) -> &mut BootInfoFrameAllocator {
        &mut self.frame_allocator
    }

    pub(crate) unsafe fn page_table_mut(
        &self,
        frame: PhysFrame<Size4KiB>,
    ) -> &'static mut PageTable {
        let virt = self.physical_memory_offset + frame.start_address().as_u64();
        let ptr: *mut PageTable = virt.as_mut_ptr();

        unsafe { &mut *ptr }
    }

    pub(crate) unsafe fn page_table(&self, frame: PhysFrame<Size4KiB>) -> &'static PageTable {
        let virt = self.physical_memory_offset + frame.start_address().as_u64();
        let ptr: *const PageTable = virt.as_ptr();

        unsafe { &*ptr }
    }

    pub(crate) unsafe fn frame_slice_mut(
        &self,
        frame: PhysFrame<Size4KiB>,
    ) -> &'static mut [u8; 4096] {
        let virt = self.physical_memory_offset + frame.start_address().as_u64();
        let ptr: *mut [u8; 4096] = virt.as_mut_ptr();

        unsafe { &mut *ptr }
    }

    pub(crate) unsafe fn phys_ptr<T>(&self, phys: PhysAddr) -> *const T {
        (self.physical_memory_offset + phys.as_u64()).as_ptr()
    }
}
