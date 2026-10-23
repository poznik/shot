//! The allocator: blocks of 256 KB and more are pages mapped from the system and unmapped when
//! freed; the rest come from malloc. Malloc keeps the freed blocks of its medium zone, 32 KB to
//! 8 MB, for reuse: a capture's crop, thumbnail and encoder buffers, 5–8 MB, would count as
//! Shot's memory until the next capture, against the budget of idle + 5 MB after one (MAC-005).

use std::alloc::{GlobalAlloc, Layout, System};
use std::ptr;

pub struct Allocator;

/// Blocks from this size on are mapped.
const BIG: usize = 256 * 1024;
/// The alignment a mapping always has; Apple Silicon's pages are larger.
const PAGE: usize = 4096;

fn big(layout: &Layout) -> bool {
    layout.size() >= BIG && layout.align() <= PAGE
}

fn map(size: usize) -> *mut u8 {
    // SAFETY: a fresh private anonymous mapping, zero-filled.
    let p = unsafe {
        libc::mmap(
            ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };
    if p == libc::MAP_FAILED {
        ptr::null_mut()
    } else {
        p.cast()
    }
}

/// The size a mapping of `size` bytes really has.
fn mapped(size: usize) -> usize {
    size.next_multiple_of(page())
}

fn page() -> usize {
    // SAFETY: no arguments to get wrong.
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if big(&layout) {
            map(layout.size())
        } else {
            // SAFETY: the caller's contract.
            unsafe { System.alloc(layout) }
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if big(&layout) {
            map(layout.size())
        } else {
            // SAFETY: the caller's contract.
            unsafe { System.alloc_zeroed(layout) }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if big(&layout) {
            // SAFETY: a mapping made by `alloc` for this layout.
            unsafe { libc::munmap(ptr.cast(), layout.size()) };
        } else {
            // SAFETY: the caller's contract.
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller's contract makes the new layout valid.
        let new = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        match (big(&layout), big(&new)) {
            // SAFETY: the caller's contract.
            (false, false) => unsafe { System.realloc(ptr, layout, new_size) },
            // The pages the mapping has hold it.
            (true, true) if mapped(new_size) == mapped(layout.size()) => ptr,
            _ => {
                // SAFETY: `new` is valid, as above.
                let to = unsafe { self.alloc(new) };
                if !to.is_null() {
                    // SAFETY: both blocks hold the smaller size and do not overlap.
                    unsafe {
                        ptr::copy_nonoverlapping(ptr, to, layout.size().min(new_size));
                        self.dealloc(ptr, layout);
                    }
                }
                to
            }
        }
    }
}
