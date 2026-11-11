// Fixture for the syn visitor tests. Not compiled.
use kernel::prelude::*;
use kernel::{c_str, sync::{Arc, Mutex}};
use ::kernel::device;
use core::ptr;

const MAX: usize = 4;
static COUNT: u32 = 0;
type Res = Result<(), ()>;

struct Foo {
    a: u32,
}

enum Kind {
    A,
    B,
}

union Bits {
    i: u32,
    f: f32,
}

trait Shape {
    fn area(&self) -> u32;
    unsafe fn raw(&self) -> *const u8;
}

unsafe trait Marker {}

impl Foo {
    fn new() -> Self {
        // SAFETY: the lock is valid for the lifetime of `Foo`.
        unsafe { bindings::mutex_lock(ptr::null_mut()) };
        Foo { a: 0 }
    }
}

impl Shape for Foo {
    fn area(&self) -> u32 {
        let x = unsafe { kernel::bindings::mutex_lock(ptr::null_mut()) };
        self.a
    }
    unsafe fn raw(&self) -> *const u8 {
        ptr::null()
    }
}

unsafe impl Marker for Foo {}

fn helper() {
    // SAFETY: Two lines of
    // justification, then the block.
    let v = unsafe {
        crate::bindings::kfree(ptr::null_mut());
        1
    };
    something.bindings();
    bindings_like::call();
    let w = bindings::wait_queue_head::default();
}

mod inner {
    fn inner_fn() {}
}

unsafe fn unsafe_free() {
    // Not a safety comment.
    unsafe { core::hint::unreachable_unchecked() }
}

macro_rules! twice {
    ($e:expr) => {
        unsafe { $e; $e }
    };
}

#[proc_macro_derive(Thing)]
fn derive_it() {}
