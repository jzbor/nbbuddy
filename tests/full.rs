extern crate alloc;

use nbbuddy::*;
use core::ptr;
use alloc::alloc::GlobalAlloc;
use alloc::alloc::Layout;

const ALLOC_SIZE: usize = 8 * 1024;
const SMALLEST_CHUNK: usize = 64;
static ALLOCATOR: nbbuddy::NBBuddyAllocator<ALLOC_SIZE, { nnodes!(ALLOC_SIZE, SMALLEST_CHUNK) }> = NBBuddyAllocator::new();

#[test]
fn full() {
    unsafe {
        let mut pointers: [*mut u8; ALLOC_SIZE / SMALLEST_CHUNK] = [ptr::null_mut(); ALLOC_SIZE / SMALLEST_CHUNK];
        let layout = Layout::from_size_align(SMALLEST_CHUNK, SMALLEST_CHUNK).unwrap();

        // allocate pointers
        for i in 0..(ALLOC_SIZE / SMALLEST_CHUNK) {
            pointers[i] = ALLOCATOR.alloc(layout);
            assert!(!pointers[i].is_null());
        }

        // check if everything was allocated
        let additional_pointer = ALLOCATOR.alloc(layout);
        assert!(additional_pointer.is_null());

        // deallocate pointers
        for i in 0..(ALLOC_SIZE / SMALLEST_CHUNK) {
            ALLOCATOR.dealloc(pointers[i], layout);
        }
    }
}
