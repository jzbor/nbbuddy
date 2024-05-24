extern crate alloc;

use nbbuddy::*;
use alloc::alloc::GlobalAlloc;
use alloc::alloc::Layout;

const ALLOC_SIZE: usize = 8 * 1024 * 1024;
static ALLOCATOR: nbbuddy::NBBuddyAllocator<ALLOC_SIZE, { nnodes!(ALLOC_SIZE, 128) }> = NBBuddyAllocator::new();

#[test]
fn basic() {
    unsafe {
        let pointer = ALLOCATOR.alloc(Layout::new::<u64>());
        assert!(!pointer.is_null());
        ALLOCATOR.dealloc(pointer, Layout::new::<u64>());
    }
}
