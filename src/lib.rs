#![no_std]

extern crate alloc as core_alloc;

use core_alloc::alloc;
use core::ptr;
use core::cmp;
use core::sync::atomic::{AtomicU8, Ordering};

#[macro_export]
macro_rules! nnodes {
    ($size:expr, $smallest_chunk:expr) => { 2 * ($size / $smallest_chunk) }
}

pub struct NBBuddyAllocator<const S: usize, const N: usize> {
    memory: [u8; S],
    smallest_chunk: usize,
    tree: [AtomicU8; N],
    index: [usize; N],
    reservation: [usize; N],
}

#[repr(u8)]
enum AllocTags {
    OccRight    = 0x1,
    OccLeft     = 0x2,
    CoalRight   = 0x4,
    CoalLeft    = 0x8,
    Occ         = 0x10,
    Busy        = AllocTags::Occ as u8 | AllocTags::OccLeft as u8 | AllocTags::OccRight as u8,
}

impl<const S: usize, const N: usize> NBBuddyAllocator<S, N> {
    pub const fn new() -> Self {
        const TREE_INIT_VALUE: AtomicU8 = AtomicU8::new(0);
        NBBuddyAllocator {
            memory: [0; S],
            smallest_chunk: S / (1 << (usize::ilog2(N + 1) - 1)),
            tree: [TREE_INIT_VALUE; N],
            index: [0; N],
            reservation: [0; N],
        }
    }

    fn try_alloc(&self, n: usize) -> Result<(), usize> {
        let empty = 0;
        if self.tree[n].compare_exchange(empty, AllocTags::Busy as u8, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err(n);
        }

        let mut current = n;
        while level(current) != 0 {
            let child = current;
            current = current >> 1;
            loop {
                let cur_val = self.tree[current].load(Ordering::SeqCst);
                if cur_val & AllocTags::Occ as u8 != 0 {
                    self.free_node(n, level(child));
                    return Err(current);
                }
                let new_val = AllocTags::mark(AllocTags::clean_coal(cur_val, child), child);
                if self.cas_tree_value(current, cur_val, new_val).is_ok() {
                    break;
                }
            };
        }

        Ok(())
    }

    fn free_node(&self, n: usize, upper_bound: usize) {
        let mut current = parent(n);
        let mut runner = n;
        while level(runner) > upper_bound {
            let or_val = if lchild(current) == runner { (AllocTags::CoalRight as u8) << 1 } else { AllocTags::CoalRight as u8 };
            let old_val = self.tree[current].fetch_or(or_val, Ordering::SeqCst);

            if AllocTags::is_occ_buddy(old_val, runner) && !AllocTags::is_coal_buddy(old_val, runner) {
                break;
            }

            runner = current;
            current = parent(current);
        }

        self.tree[n].store(0, Ordering::SeqCst);
        if n != upper_bound {
            self.unmark(n, upper_bound);
        }
    }

    fn unmark(&self, n: usize, upper_bound: usize) {
        let mut current = n;
        loop {
            let child = current;
            current = current >> 1;
            let new_val = loop {
                let cur_val = self.tree[current].load(Ordering::SeqCst);
                if !AllocTags::is_coal(cur_val, child) {
                    return;
                }
                let new_val = AllocTags::unmark(cur_val, child);
                if self.cas_tree_value(current, cur_val, new_val).is_ok() {
                    break new_val;
                }
            };
            if level(current) > upper_bound && !AllocTags::is_occ_buddy(new_val, child) {
                break;
            }
        }
    }

    fn cas_tree_value(&self, i: usize, cur_val: u8, new_val: u8) -> Result<u8, u8> {
        self.tree[i].compare_exchange(cur_val, new_val, Ordering::SeqCst, Ordering::SeqCst)
    }

    fn depth(&self) -> usize {
        usize::ilog2(S / self.smallest_chunk) as usize
    }
}

impl AllocTags {
    fn clean_coal(v: u8, c: usize) -> u8 {
        v & !(Self::CoalLeft as u8 >> (c % 2))
    }

    fn mark(v: u8, c: usize) -> u8 {
        v | (Self::OccLeft as u8 >> (c % 2))
    }

    fn unmark(v: u8, c: usize) -> u8 {
        v & !((Self::OccLeft as u8 | Self::CoalLeft as u8) >> (c % 2))
    }

    fn is_coal(v: u8, c: usize) -> bool {
        v & (Self::CoalLeft as u8 >> (c % 2)) != 0
    }

    fn is_occ_buddy(v: u8, c: usize) -> bool {
        v & ((Self::OccRight as u8) << (c % 2)) != 0
    }

    fn is_coal_buddy(v: u8, c: usize) -> bool {
        v & ((Self::CoalRight as u8) << (c % 2)) != 0
    }

    fn is_free(v: u8) -> bool {
        !(v & Self::Busy as u8) != 0
    }
}

unsafe impl<const S: usize, const N: usize> alloc::GlobalAlloc for NBBuddyAllocator<S, N> {
    unsafe fn alloc(&self, layout: alloc::Layout) -> *mut u8 {
        if layout.size() > S || layout.size() == 0 {
            return ptr::null_mut()
        }

        let req_level = cmp::min(level(S / layout.size()), self.depth());
        let start_from = 1 << req_level;
        let until_to = 1 << (req_level + 1);
        let base_addr = ptr::addr_of!(self.memory) as usize;

        let mut i = start_from;
        while i < until_to {
            if AllocTags::is_free(self.tree[i].load(Ordering::SeqCst)) {
                let result = self.try_alloc(i);
                if let Err(failed_at) = result {
                    let d = 1 << (level(i) - level(failed_at));
                    i = (failed_at + 1) * d;
                    i -= 1;  // seems to be required to avoid skipping buddy
                } else {
                    *(ptr::addr_of!(self.index[(starting(i, base_addr, S) - base_addr) / self.smallest_chunk]) as *mut usize) = i;
                    *(ptr::addr_of!(self.reservation[i]) as *mut usize) = layout.size();
                    return starting(i, base_addr, S) as *mut u8;
                }
            }
            i += 1
        }

        ptr::null_mut()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: alloc::Layout) {
        if ptr.is_null() { return }
        let n = self.index[(ptr as usize - ptr::addr_of!(self.memory) as usize) / self.smallest_chunk];
        self.free_node(n, 0);
    }
}

const fn level(n: usize) -> usize { usize::ilog2(n) as usize }
const fn parent(n: usize) -> usize { n >> 1 }
const fn lchild(n: usize) -> usize { n << 1 }
// const fn rchild(n: usize) -> usize { lchild(n) + 1 }
const fn size(n: usize, t: usize) -> usize { t / (1 << level(n)) }
const fn starting(n: usize, b: usize, t: usize) -> usize { b + (n - (1 << level(n))) * size(n, t) }

